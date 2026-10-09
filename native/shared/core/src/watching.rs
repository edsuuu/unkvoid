//! Assistir sem GStreamer: o RTP aberto vira quadro H.264 e PCM, e a interface decodifica
//! com o que o sistema tem (VideoToolbox no macOS, Media Foundation no Windows).
//!
//! É o mesmo desenho do `watching.rs` do Linux — um `PlainReceiver` para a sala inteira, uma
//! rota por producer —, trocando o `gst-launch` filho por uma thread que remonta o quadro.
//! O que sai daqui vai para uma fila só, e a interface a esvazia no ritmo dela.

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use media::{AudioUnpacker, Counters, PlainReceiver, Rtx, Stream, VideoUnpacker};
use serde_json::Value;

/// Quadros e blocos de som esperando a interface. Dois segundos de uma tela a 60 fps com o
/// som junto; passou disso a interface não está acompanhando, e guardar mais só atrasaria.
const QUEUE: usize = 256;

/// De quanto em quanto tempo a thread olha se mandaram parar.
const PATIENCE: Duration = Duration::from_millis(200);

/// O maior datagrama que o `PlainReceiver` repassa.
const DATAGRAM: usize = 1_500;

/// De quanto em quanto tempo a imagem parada pede outro quadro-chave: o pedido pode se perder,
/// ou cair no espaço que quem transmite deixa entre dois.
const ASK_AGAIN: Duration = Duration::from_secs(1);

/// Parada mais curta que isto não vai para o log: o quadro-chave pedido chega em ~meio segundo.
pub const WORTH_TELLING: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// `timestamp` no relógio de 90 kHz do RTP.
    Video { keyframe: bool, timestamp: u32 },
    /// PCM `f32` little-endian, estéreo intercalado, 48 kHz.
    Audio,
}

/// Um quadro H.264 em Annex-B ou um bloco de som, de um producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub producer_id: String,
    pub kind: MediaKind,
    pub data: Vec<u8>,
    /// Quando ele ficou pronto aqui, saído da rede. É a chegada que o jitter buffer de quem
    /// assiste mede: carimbada depois, na fila da interface, a CPU ocupada pelo jogo virava
    /// atraso de rede, e a espera subia a meio segundo por um tranco que a rede nem teve.
    pub arrived: Instant,
    /// O vídeo que este som acompanha: o som da tela segue a tela. Quem toca o segura o tanto
    /// que a imagem dele espera no `Playout`, senão numa rede com perda a imagem saía até meio
    /// segundo depois do som dela.
    pub follows: Option<String>,
}

/// O que o `consumePlain` respondeu sobre uma transmissão.
pub struct Incoming<'a> {
    pub producer_id: String,
    pub kind: &'a str,
    pub address: &'a str,
    pub server_key: &'a [u8],
    pub payload_type: u8,
    /// O que o servidor devolveu; sem ele o receptor aprende no primeiro pacote.
    pub ssrc: Option<u32>,
    /// Som de tela compartilhada, que chega mudo por regra.
    pub always_muted: bool,
    /// A retransmissão do servidor: é por ela que pacote perdido volta.
    pub rtx: Option<Rtx>,
    /// O vídeo que este som acompanha — ver `Media::follows`.
    pub follows: Option<String>,
}

/// O `rtx` da resposta do `consumePlain`, quando o servidor anuncia um.
pub fn rtx_of(answer: &Value) -> Option<Rtx> {
    Some(Rtx {
        ssrc: u32::try_from(answer["rtx"]["ssrc"].as_u64()?).ok()?,
        payload_type: u8::try_from(answer["rtx"]["payloadType"].as_u64()?).ok()?,
    })
}

struct Watch {
    stop: Arc<AtomicBool>,
    video: bool,
    always_muted: bool,
}

pub struct Watching {
    /// A chave SRTP deste lado, uma só para o app inteiro.
    key: Option<[u8; 30]>,
    /// O socket da sessão, aberto no primeiro producer e fechado com o último.
    receiver: Option<PlainReceiver>,
    active: HashMap<String, Watch>,
    deafened: bool,
    /// O que a pessoa escolheu calar (`true`) ou ouvir em cada som: o da tela que ela ligou, o
    /// microfone de alguém que ela mutou. Guardado fora da rota porque a rota é refeita — o
    /// caminho de chegada refeito, a volta de uma queda, o som que chega depois do clique — e a
    /// rota nova nascia com a regra (o som da tela mudo, o microfone ligado), calada para a tela
    /// que mostrava a escolha.
    chosen: HashMap<String, bool>,
    out: SyncSender<Media>,
}

impl Watching {
    /// Devolve também a ponta que a interface esvazia.
    pub fn new() -> (Self, Receiver<Media>) {
        let (out, queue) = sync_channel(QUEUE);

        (
            Self {
                key: None,
                receiver: None,
                active: HashMap::new(),
                deafened: false,
                chosen: HashMap::new(),
                out,
            },
            queue,
        )
    }

    /// A chave que vai ao servidor no `consumePlain`.
    pub fn key(&mut self) -> [u8; 30] {
        *self
            .key
            .get_or_insert_with(media::PlainSender::generate_key)
    }

    /// Fecha tudo e sorteia outra chave: é ela que faz o servidor abrir um transporte de
    /// chegada novo no lugar do que o `comedia` prendeu a um endereço que não vale mais.
    pub fn renew(&mut self) {
        self.stop(None);
        self.key = None;
    }

    pub fn is_watching(&self, producer_id: &str) -> bool {
        self.active.contains_key(producer_id)
    }

    pub fn start(&mut self, incoming: Incoming<'_>) -> Result<()> {
        let Incoming {
            producer_id,
            kind,
            address,
            server_key,
            payload_type,
            ssrc,
            always_muted,
            rtx,
            follows,
        } = incoming;

        // Outro endereço é outra sessão no servidor: o que estava aberto já morreu lá.
        if self
            .receiver
            .as_ref()
            .is_some_and(|receiver| media::resolve(address).ok() != Some(receiver.server()))
        {
            self.stop(None);
        }

        if self.active.contains_key(&producer_id) {
            return Ok(());
        }

        let key = self.key();

        if self.receiver.is_none() {
            self.receiver = Some(
                PlainReceiver::start(address, &key, server_key)
                    .map_err(|error| anyhow!("{error}"))?,
            );
        }

        let receiver = self.receiver.as_ref().ok_or_else(|| anyhow!("sem receptor"))?;
        let socket =
            UdpSocket::bind("127.0.0.1:0").context("sem porta local para a transmissão")?;
        let to = socket.local_addr()?;
        let video = kind == "video";
        let stop = Arc::new(AtomicBool::new(false));

        socket.set_read_timeout(Some(PATIENCE))?;
        media::grow_receive_buffer(&socket);
        pump(
            socket,
            (producer_id.clone(), follows),
            video,
            Arc::clone(&stop),
            (self.out.clone(), receiver.keyframe_asker(producer_id.clone())),
        )?;
        receiver.route(Stream {
            id: producer_id.clone(),
            payload_type,
            to,
            ssrc,
            video,
            rtx,
        });

        if !video && silenced(self.chosen.get(&producer_id).copied(), always_muted, self.deafened) {
            receiver.set_muted(&producer_id, true);
        }

        tracing::info!(producer = %producer_id, kind, "assistindo por RTP puro");
        self.active.insert(
            producer_id,
            Watch {
                stop,
                video,
                always_muted,
            },
        );

        Ok(())
    }

    /// `None` fecha a sessão inteira. O socket só sai nesse caso: o SFU manda para o
    /// endereço que o `comedia` aprendeu, e um socket novo não recebe mais nada naquela sala.
    pub fn stop(&mut self, producer_id: Option<&str>) {
        let keys: Vec<String> = match producer_id {
            Some(producer_id) => vec![producer_id.to_owned()],
            None => self.active.keys().cloned().collect(),
        };

        for key in keys {
            if let Some(watch) = self.active.remove(&key) {
                watch.stop.store(true, Ordering::Relaxed);
            }

            // O producer que fechou não volta: a escolha dele vai junto. Refazer o caminho
            // (`None`) não fecha producer nenhum, e as escolhas ficam.
            if producer_id.is_some() {
                self.chosen.remove(&key);
            }

            if let Some(receiver) = self.receiver.as_ref() {
                receiver.unroute(&key);
            }
        }

        if producer_id.is_none() {
            self.receiver = None;
        }
    }

    /// A escolha da pessoa de calar (ou ouvir) um som, que vale também para a rota que ainda
    /// não existe ou que for refeita. Mudo é não repassar o pacote: o decodificador só vê
    /// silêncio. Surda, o som continua calado até ela voltar a ouvir.
    pub fn set_muted(&mut self, producer_id: &str, muted: bool) {
        self.chosen.insert(producer_id.to_owned(), muted);

        if let Some(receiver) = self.receiver.as_ref() {
            receiver.set_muted(producer_id, muted || self.deafened);
        }
    }

    /// Se o som de um producer está calado aqui agora. `None` sem rota para ele.
    pub fn is_muted(&self, producer_id: &str) -> Option<bool> {
        self.receiver.as_ref()?.is_muted(producer_id)
    }

    /// Ensurdecer cala só o áudio: pausar o vídeo faria esperar keyframe na volta. Voltar a
    /// ouvir devolve cada som à escolha da pessoa, e não à regra.
    pub fn deafen(&mut self, deafened: bool) {
        self.deafened = deafened;

        let Some(receiver) = self.receiver.as_ref() else {
            return;
        };

        for (producer_id, watch) in &self.active {
            if !watch.video {
                receiver.set_muted(producer_id, silenced(self.chosen.get(producer_id).copied(), watch.always_muted, deafened));
            }
        }
    }

    pub fn is_deafened(&self) -> bool {
        self.deafened
    }

    /// O que aconteceu com o vídeo de uma transmissão: recebidos, recuperados e perdidos.
    pub fn counters(&self, producer_id: &str) -> Option<Counters> {
        self.receiver.as_ref()?.counters(producer_id)
    }

    /// Quem decodifica perdeu o fio: o keyframe é pedido agora ao servidor.
    pub fn request_keyframe(&self, producer_id: &str) {
        if let Some(receiver) = &self.receiver {
            receiver.request_keyframe(producer_id);
        }
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.stop(None);
    }
}

/// Se um som fica calado: a escolha da pessoa vale sobre a regra (o som da tela chega mudo), e
/// surdo cala tudo.
fn silenced(chosen: Option<bool>, always_muted: bool, deafened: bool) -> bool {
    deafened || chosen.unwrap_or(always_muted)
}

/// A thread de um producer: lê o RTP que o receptor repassou e põe na fila o que remontou.
fn pump<Ask: Fn() + Send + 'static>(
    socket: UdpSocket,
    (producer_id, follows): (String, Option<String>),
    video: bool,
    stop: Arc<AtomicBool>,
    (out, ask): (SyncSender<Media>, Ask),
) -> Result<()> {
    let audio = if video {
        None
    } else {
        Some(AudioUnpacker::new()?)
    };

    std::thread::Builder::new()
        .name(format!("watch-{producer_id}"))
        .spawn(move || match audio {
            Some(audio) => pump_audio(&socket, (&producer_id, follows.as_deref()), &stop, &out, audio),
            None => pump_video(&socket, &producer_id, &stop, &out, &ask),
        })?;

    Ok(())
}

fn pump_audio(socket: &UdpSocket, (producer_id, follows): (&str, Option<&str>), stop: &AtomicBool, out: &SyncSender<Media>, mut audio: AudioUnpacker) {
    let mut datagram = [0_u8; DATAGRAM];

    while !stop.load(Ordering::Relaxed) {
        let Some(samples) = socket.recv(&mut datagram).ok().and_then(|size| audio.push(&datagram[..size])) else {
            continue;
        };

        let media = Media {
            producer_id: producer_id.to_owned(),
            kind: MediaKind::Audio,
            data: samples.iter().flat_map(|sample| sample.to_le_bytes()).collect(),
            arrived: Instant::now(),
            follows: follows.map(str::to_owned),
        };

        if let Err(TrySendError::Disconnected(_)) = out.try_send(media) {
            return;
        }
    }
}

/// O vídeo, com o que a recuperação do receptor não vê: o pacote perdido no repasse local e o
/// quadro que ficou de fora da fila cheia também deixam a imagem esperando um quadro-chave, e é
/// daqui que ele é pedido — de novo a cada `ASK_AGAIN` enquanto não vem. Antes ninguém pedia, e
/// a tela ficava parada até o quadro-chave periódico, 4 s depois.
fn pump_video(socket: &UdpSocket, producer_id: &str, stop: &AtomicBool, out: &SyncSender<Media>, ask: &impl Fn()) {
    let mut frames = VideoUnpacker::default();
    let mut stalled = Stalled::default();
    // A fila da interface encheu e um quadro ficou de fora: o P seguinte desenharia lixo.
    let mut broken = false;
    let mut datagram = [0_u8; DATAGRAM];

    while !stop.load(Ordering::Relaxed) {
        let unit = socket.recv(&mut datagram).ok().and_then(|size| frames.push(&datagram[..size]));
        let now = Instant::now();

        if let Some(unit) = unit
            && (!broken || unit.keyframe)
        {
            let media = Media {
                producer_id: producer_id.to_owned(),
                kind: MediaKind::Video { keyframe: unit.keyframe, timestamp: unit.timestamp },
                data: unit.data,
                arrived: now,
                follows: None,
            };

            match out.try_send(media) {
                Ok(()) => {
                    broken = false;

                    if let Some(lasted) = stalled.flowing(now)
                        && lasted >= WORTH_TELLING
                    {
                        tracing::error!(producer = producer_id, seconds = lasted.as_secs_f32(), "assistir: a imagem ficou parada esperando um quadro-chave");
                    }
                }
                Err(TrySendError::Full(_)) => broken = true,
                Err(TrySendError::Disconnected(_)) => return,
            }
        }

        if (broken || frames.waiting_keyframe()) && stalled.waiting(now) {
            ask();
        }
    }
}

/// Quanto a tela que o servidor diz estar recebendo pode ficar sem chegar aqui antes de o
/// caminho de chegada ser dado como morto. Acima do GOP de 4 s: o consumer recém-aberto espera
/// um quadro-chave antes do primeiro pacote.
const RECEIVE_SILENCE: Duration = Duration::from_secs(5);

/// O menor espaço entre dois refazer do caminho de chegada. Dobra a cada um que não resolveu
/// dentro de `MOST_REWATCH_SPACING`, e volta ao começo depois disso.
const REWATCH_SPACING: Duration = Duration::from_secs(10);
const MOST_REWATCH_SPACING: Duration = Duration::from_secs(60);

/// O vigia do caminho de chegada. De segundo em segundo recebe, de cada tela assistida (e não
/// pausada), quantos pacotes chegaram até agora e se o servidor diz estar recebendo dela, e
/// diz quando refazer o caminho: o servidor recebe e nada chega aqui há `RECEIVE_SILENCE`.
/// Lógica pura, com o relógio passado por quem chama.
#[derive(Debug)]
pub struct ArrivalWatch {
    seen: HashMap<String, (u64, Instant)>,
    rewatched_at: Option<Instant>,
    spacing: Duration,
}

impl Default for ArrivalWatch {
    fn default() -> Self {
        Self { seen: HashMap::new(), rewatched_at: None, spacing: REWATCH_SPACING }
    }
}

impl ArrivalWatch {
    pub fn tick(&mut self, screens: &[(String, u64, bool)], now: Instant) -> bool {
        self.seen.retain(|producer, _| screens.iter().any(|(id, ..)| id == producer));

        let mut dead = false;

        for (producer, packets, receiving) in screens {
            let seen = self.seen.entry(producer.clone()).or_insert((*packets, now));

            if *packets != seen.0 || !receiving {
                *seen = (*packets, now);

                continue;
            }

            dead |= now.duration_since(seen.1) >= RECEIVE_SILENCE;
        }

        if !dead {
            return false;
        }

        if let Some(at) = self.rewatched_at {
            let since = now.duration_since(at);

            if since < self.spacing {
                return false;
            }

            self.spacing = if since < MOST_REWATCH_SPACING { (self.spacing * 2).min(MOST_REWATCH_SPACING) } else { REWATCH_SPACING };
        }

        self.rewatched_at = Some(now);
        self.seen.clear();

        true
    }
}

/// A imagem de uma transmissão parada à espera de um quadro-chave: desde quando, e quando o
/// último foi pedido.
#[derive(Debug, Default)]
pub struct Stalled {
    since: Option<Instant>,
    asked: Option<Instant>,
    /// Já saiu imagem alguma vez: a espera da primeira não é parada.
    flowed: bool,
}

impl Stalled {
    /// Ainda esperando: diz se é hora de pedir (de novo).
    pub fn waiting(&mut self, now: Instant) -> bool {
        self.since.get_or_insert(now);

        let due = self.asked.is_none_or(|asked| now.duration_since(asked) >= ASK_AGAIN);

        if due {
            self.asked = Some(now);
        }

        due
    }

    /// Saiu imagem: quanto durou a parada, se houve uma depois da primeira imagem.
    pub fn flowing(&mut self, now: Instant) -> Option<Duration> {
        self.asked = None;

        let since = self.since.take();

        std::mem::replace(&mut self.flowed, true)
            .then_some(since)
            .flatten()
            .map(|since| now.duration_since(since))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming<'a>(producer_id: &str, kind: &'a str, address: &'a str, server_key: &'a [u8], always_muted: bool) -> Incoming<'a> {
        Incoming { producer_id: producer_id.to_owned(), kind, address, server_key, payload_type: 111, ssrc: Some(7), always_muted, rtx: None, follows: None }
    }

    /// O som da tela que a pessoa ligou antes de ele chegar continua ligado quando chega, e
    /// quando o caminho é refeito; o microfone de quem ela mutou continua mudo depois de ela
    /// ensurdecer e voltar a ouvir. Antes, a rota nova nascia com a regra e a volta do surdo
    /// desmutava todo mundo.
    #[test]
    fn the_choice_to_hear_or_mute_a_sound_survives_the_route_being_rebuilt() {
        let server = UdpSocket::bind("127.0.0.1:0").expect("um servidor de mentira");
        let address = server.local_addr().expect("porta").to_string();
        let server_key = media::PlainSender::generate_key();
        let (mut watching, _queue) = Watching::new();

        watching.set_muted("som-da-tela", false);
        watching.start(incoming("som-da-tela", "audio", &address, &server_key, true)).expect("assistiu");
        watching.start(incoming("microfone", "audio", &address, &server_key, false)).expect("assistiu");

        assert_eq!(watching.is_muted("som-da-tela"), Some(false), "o som ligado antes de chegar chegou mudo");

        watching.set_muted("microfone", true);
        watching.deafen(true);

        assert_eq!((watching.is_muted("som-da-tela"), watching.is_muted("microfone")), (Some(true), Some(true)), "surdo cala tudo");

        watching.deafen(false);

        assert_eq!(watching.is_muted("som-da-tela"), Some(false), "voltar a ouvir calou o som da tela");
        assert_eq!(watching.is_muted("microfone"), Some(true), "voltar a ouvir desmutou quem a pessoa mutou");

        watching.renew();
        watching.start(incoming("som-da-tela", "audio", &address, &server_key, true)).expect("assistiu de novo");
        watching.start(incoming("microfone", "audio", &address, &server_key, false)).expect("assistiu de novo");

        assert_eq!(watching.is_muted("som-da-tela"), Some(false), "o caminho refeito calou o som da tela");
        assert_eq!(watching.is_muted("microfone"), Some(true), "o caminho refeito desmutou quem a pessoa mutou");
    }

    /// O pedido sai na hora, repete a cada segundo enquanto a imagem não volta, e a parada só
    /// conta depois da primeira imagem.
    #[test]
    fn a_stalled_picture_asks_again_and_tells_how_long_it_stood() {
        let start = Instant::now();
        let at = |millis: u64| start + Duration::from_millis(millis);
        let mut stalled = Stalled::default();

        assert!(stalled.waiting(at(0)), "a espera do primeiro quadro pede");
        assert_eq!(stalled.flowing(at(300)), None, "a primeira imagem não é parada");

        assert!(stalled.waiting(at(1_000)));
        assert!(!stalled.waiting(at(1_500)), "meio segundo depois ainda não");
        assert!(stalled.waiting(at(2_000)), "um segundo depois pede de novo");
        assert_eq!(stalled.flowing(at(3_000)), Some(Duration::from_secs(2)));
        assert!(stalled.waiting(at(3_100)), "a próxima parada pede na hora");
    }

    /// Só é caminho morto com o servidor recebendo e nada chegando aqui por 5 s; a tela parada
    /// de quem transmite não conta, e o segundo refazer espera o espaço dele.
    #[test]
    fn the_arrival_path_is_rebuilt_only_when_the_server_receives_and_nothing_comes() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let screen = |packets: u64, receiving: bool| [("tela".to_owned(), packets, receiving)];
        let mut arrival = ArrivalWatch::default();

        assert!(!arrival.tick(&screen(10, true), at(0)));
        assert!(!arrival.tick(&screen(10, false), at(6)), "quem transmite parou: nada a refazer");
        assert!(!arrival.tick(&screen(10, true), at(7)));
        assert!(!arrival.tick(&screen(10, true), at(10)), "quatro segundos ainda é espera de quadro-chave");
        assert!(arrival.tick(&screen(10, true), at(11)), "cinco segundos recebendo lá e nada aqui");

        assert!(!arrival.tick(&screen(0, true), at(12)));
        assert!(!arrival.tick(&screen(0, true), at(17)), "dentro dos 10 s do refazer anterior");
        assert!(arrival.tick(&screen(0, true), at(21)));
    }
}
