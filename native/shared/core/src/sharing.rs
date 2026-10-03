//! Transmitir: liga captura, encoder e transporte.
//!
//! `captura → textura na GPU → encoder de hardware → 1 quadro → SFU → N espectadores`. O
//! quadro não desce para a memória do processador antes de ser comprimido, é comprimido uma
//! vez e sobe uma vez. Mudança que quebre uma dessas três coisas desfaz o projeto.
//!
//! Mora no núcleo, e não em cada pasta de sistema, porque o que ele decide é o mesmo nos
//! três: qual origem, qual taxa, quando republicar, quando o encoder da placa não serve. O
//! que muda de sistema para sistema já está em `capture` e `media`.
//!
//! No Linux a captura entrega H.264 pronto (quem comprime é o GStreamer); no macOS e no
//! Windows ela entrega a textura e o encoder da placa é um passo à parte. As duas formas
//! passam por aqui.

use std::sync::{
    Arc, Mutex, OnceLock, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer};
use media::{AudioEncoder, BitrateGovernor, EncoderConfig, PlainSender, PlatformEncoder, Source};

/// De quanto em quanto tempo o governador da taxa recebe os números. Um segundo junta uns
/// mil pacotes em 1080p60, amostra em que 5% de perda quer dizer alguma coisa. O encoder
/// leva bem mais do que isso para chegar a uma taxa nova; quem espera por ele é a carência
/// do governador, não esta janela.
const RATE_WINDOW: Duration = Duration::from_secs(1);

/// O encoder de vídeo e quem decide a taxa dele, atrás do mesmo cadeado: a decisão é
/// aplicada na thread da captura, a única que toca no encoder, no cadeado que o quadro já
/// tomaria de qualquer jeito.
/// O menor espaço entre dois quadros-chave pedidos por quem assiste. Cada pessoa que perde um
/// pacote pede um, e com várias assistindo os pedidos se somam ao GOP: medido em 27/09 na tela
/// de alguém com PC e upload fracos, saía um quadro-chave por segundo. É o quadro mais caro do
/// encoder, e num upload fraco cada um entope a saída por centenas de ms — os quadros de trás
/// esperam, e para quem assiste a transmissão trava. Pedido dentro do intervalo não se perde:
/// sai quando o intervalo acaba.
const KEYFRAME_SPACING: Duration = Duration::from_secs(2);

/// Quanto a captura pode ficar sem quadro antes de ser refeita. Tela parada também não manda
/// quadro (o Windows só entrega quando algo muda), então a espera dobra a cada vez que a
/// captura refeita volta a calar, até `MOST_WAIT`: numa tela parada isso vira um quadro-chave
/// de tempos em tempos, e numa captura travada a imagem volta em segundos.
const CAPTURE_WAIT: Duration = Duration::from_secs(3);

/// Captura chegando e encoder sem devolver nada por isto é encoder travado. Medido em 02/10 na
/// tela do mank: 4 s a 60 fps e depois 25 s sem um pacote, com a tela no ar e a rede limpa — e
/// o quadro que o encoder não devolvia só ia para o log em nível de depuração, que não é gravado.
const ENCODER_WAIT: Duration = Duration::from_secs(2);

/// As esperas dobram a cada refeita que não resolve, e param aqui.
const MOST_WAIT: Duration = Duration::from_secs(60);

/// Quadros seguidos que provam que a etapa refeita está viva: a espera volta ao começo.
const ALIVE_FRAMES: u64 = 120;

/// Os contadores de uma transmissão, de onde o vigia tira se alguma etapa parou.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub captured: u64,
    pub encoded: u64,
    pub sent: u64,
}

/// A etapa que parou de produzir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stall {
    /// A captura calou: tela parada ou captura travada. Refazer custa um quadro-chave.
    Capture,
    /// A captura chega e o encoder não devolve nada.
    Encoder,
    /// O encoder devolve e nada sai para a rede: refazer não ajuda, só vai para o log.
    Transport,
}

/// O vigia de uma transmissão: de segundo em segundo recebe os contadores e diz que etapa
/// parou. Lógica pura, com o relógio passado por quem chama.
#[derive(Debug)]
pub struct StallWatch {
    last: Counts,
    captured_at: Instant,
    encoded_at: Instant,
    sent_at: Instant,
    capture_wait: Duration,
    encoder_wait: Duration,
    captured_since_restart: u64,
    encoded_since_restart: u64,
    transport_told: bool,
    /// Paradas do encoder seguidas, sem os `ALIVE_FRAMES` que provam que ele voltou.
    encoder_stalls: u32,
}

impl StallWatch {
    pub fn new(now: Instant) -> Self {
        Self {
            last: Counts::default(),
            captured_at: now,
            encoded_at: now,
            sent_at: now,
            capture_wait: CAPTURE_WAIT,
            encoder_wait: ENCODER_WAIT,
            captured_since_restart: 0,
            encoded_since_restart: 0,
            transport_told: false,
            encoder_stalls: 0,
        }
    }

    /// O encoder parou duas vezes seguidas sem provar que voltou. Refazer na placa não resolve —
    /// a memória de vídeo tomada pelo jogo, as sessões do NVENC tomadas pelo OBS —, e a saída é
    /// o processador.
    pub fn encoder_keeps_failing(&self) -> bool {
        self.encoder_stalls >= 2
    }

    pub fn tick(&mut self, counts: Counts, now: Instant) -> Option<Stall> {
        if counts.captured > self.last.captured {
            self.captured_since_restart += counts.captured - self.last.captured;
            self.captured_at = now;

            if self.captured_since_restart >= ALIVE_FRAMES {
                self.capture_wait = CAPTURE_WAIT;
            }
        }

        if counts.encoded > self.last.encoded {
            self.encoded_since_restart += counts.encoded - self.last.encoded;
            self.encoded_at = now;

            if self.encoded_since_restart >= ALIVE_FRAMES {
                self.encoder_wait = ENCODER_WAIT;
                self.encoder_stalls = 0;
            }
        }

        if counts.sent > self.last.sent {
            self.sent_at = now;
            self.transport_told = false;
        }

        self.last = counts;

        let fresh = |at: Instant| now.duration_since(at) < Duration::from_secs(1);

        if fresh(self.captured_at) && now.duration_since(self.encoded_at) >= self.encoder_wait {
            self.encoder_wait = (self.encoder_wait * 2).min(MOST_WAIT);
            self.encoder_stalls += 1;

            return Some(Stall::Encoder);
        }

        if fresh(self.encoded_at) && now.duration_since(self.sent_at) >= ENCODER_WAIT && !self.transport_told {
            self.transport_told = true;

            return Some(Stall::Transport);
        }

        if now.duration_since(self.captured_at) >= self.capture_wait {
            self.capture_wait = (self.capture_wait * 2).min(MOST_WAIT);

            return Some(Stall::Capture);
        }

        None
    }

    /// A transmissão foi refeita: os contadores do `Broadcast` novo começam do zero.
    pub fn restarted(&mut self, now: Instant) {
        self.last = Counts::default();
        self.captured_at = now;
        self.encoded_at = now;
        self.sent_at = now;
        self.captured_since_restart = 0;
        self.encoded_since_restart = 0;
    }
}

/// Até onde o espaço entre quadros-chave pedidos cresce quando os pedidos não param: é alguém
/// com perda constante pedindo um atrás do outro, e cada um é o quadro mais caro do encoder para
/// todo mundo — num upload fraco, entope a saída de quem transmite. Passar do GOP (4 s) não
/// mudaria nada: o periódico sai de qualquer jeito.
const MOST_KEYFRAME_SPACING: Duration = Duration::from_secs(4);

/// Sem pedido nenhum por isto, o espaço volta ao `KEYFRAME_SPACING`.
const KEYFRAME_QUIET: Duration = Duration::from_secs(15);

/// Os pedidos de quadro-chave de quem assiste, atendidos com espaço entre um e outro.
struct KeyframeGate {
    asked: bool,
    last: Option<Instant>,
    /// Um pedido chegou dentro do espaço e teve de esperar por ele.
    waited: bool,
    spacing: Duration,
    asked_at: Option<Instant>,
}

impl Default for KeyframeGate {
    fn default() -> Self {
        Self { asked: false, last: None, waited: false, spacing: KEYFRAME_SPACING, asked_at: None }
    }
}

impl KeyframeGate {
    /// Anota o pedido, se veio um, e diz se é hora de atender o que está esperando.
    fn due(&mut self, asked: bool, now: Instant) -> bool {
        if asked {
            self.asked_at = Some(now);
        } else if self.asked_at.is_some_and(|at| now.duration_since(at) >= KEYFRAME_QUIET) {
            self.spacing = KEYFRAME_SPACING;
        }

        self.asked |= asked;

        if !self.asked {
            return false;
        }

        if self.last.is_some_and(|last| now.duration_since(last) < self.spacing) {
            self.waited = true;

            return false;
        }

        // Pedido que esperou o espaço inteiro: eles não estão parando, e o espaço dobra.
        if std::mem::take(&mut self.waited) {
            self.spacing = (self.spacing * 2).min(MOST_KEYFRAME_SPACING);
        }

        self.served(now);

        true
    }

    /// Saiu um quadro-chave, pedido ou do GOP: quem esperava por um já tem.
    fn served(&mut self, now: Instant) {
        self.asked = false;
        self.waited = false;
        self.last = Some(now);
    }
}

struct VideoEncoding {
    encoder: PlatformEncoder,
    governor: BitrateGovernor,
    window_started: Instant,
    nacked: u64,
    keyframes: KeyframeGate,
    dropped_before: u64,
}

impl VideoEncoding {
    /// Fecha a janela: entrega os números ao governador e aplica o que ele decidir. Roda
    /// uma vez por `RATE_WINDOW`; por quadro a thread da captura só soma contadores.
    fn close_window(&mut self, now: Instant, packets: u64, dropped_total: u64) {
        let dropped = dropped_total.saturating_sub(self.dropped_before);

        self.dropped_before = dropped_total;
        self.window_started = now;

        let Some(bitrate) =
            self.governor
                .observe(packets, std::mem::take(&mut self.nacked), dropped)
        else {
            return;
        };

        if self.encoder.set_bitrate(bitrate) {
            tracing::info!(
                bitrate,
                loss_permille = self.governor.loss_permille(),
                "broadcast: a perda mudou a taxa do vídeo"
            );

            return;
        }

        self.governor.give_up();

        tracing::info!("broadcast: este encoder não troca a taxa no ar, ela fica fixa");
    }
}

/// Quantos blocos de 20 ms entram em cada nível que sai: cinco dão os dez por segundo que
/// a detecção de voz da interface espera.
const LEVEL_WINDOW_BLOCKS: u32 = 5;

/// O nível do microfone para a detecção de voz da interface. No Linux o áudio do mic não
/// passa pela janela, e sem isto ela não tem o que medir.
///
/// Sai o MAIOR bloco de cada janela, não a média: uma sílaba curta cabe num bloco de
/// 20 ms e sumiria diluída nos outros quatro.
#[derive(Default)]
struct LevelMeter {
    /// Só a voz põe alguém aqui. Tela e câmera ficam sem, e o bloco delas nem é medido.
    sink: OnceLock<Box<dyn Fn(f32) + Send + Sync>>,
    window: Mutex<(f32, u32)>,
}

impl LevelMeter {
    fn push(&self, samples: &[f32]) {
        let Some(sink) = self.sink.get() else {
            return;
        };

        let level = rms(samples);
        let mut window = self.window.lock().unwrap_or_else(PoisonError::into_inner);

        window.0 = window.0.max(level);
        window.1 += 1;

        if window.1 < LEVEL_WINDOW_BLOCKS {
            return;
        }

        let (peak, _) = std::mem::take(&mut *window);

        drop(window);
        sink(peak);
    }
}

/// A raiz da média dos quadrados, linear, de 0 (silêncio) a 1 (onda quadrada no teto).
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let power = samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;

    power.sqrt().min(1.0)
}

/// O destino, compartilhado entre quem transmite (a thread da captura) e quem o define
/// (o comando `use_sfu`, vindo da interface).
type Target = Arc<Mutex<Option<PlainSender>>>;

/// O remetente é um `Option`: uma thread que morreu com o cadeado na mão não deixa
/// estado pela metade, então o veneno é ignorado em vez de derrubar a transmissão.
fn target(sfu: &Target) -> std::sync::MutexGuard<'_, Option<PlainSender>> {
    sfu.lock().unwrap_or_else(PoisonError::into_inner)
}

/// As telas que dá para compartilhar, do jeito que a interface as mostra.
///
/// No Wayland `portal` vem `true`: lá quem escolhe a tela é o próprio sistema, e a lista
/// serve só para a interface saber que não deve desenhar um seletor próprio.
///
/// Janela é opcional: sem permissão de gravar a tela o sistema não as lista, e as telas
/// sozinhas já deixam a pessoa compartilhar.
pub fn displays() -> anyhow::Result<serde_json::Value> {
    let portal = capture::uses_system_picker();
    let found = capture::PlatformCapturer::displays()?;

    let windows = PlatformCapturer::windows().unwrap_or_default();

    Ok(serde_json::json!({
        "portal": portal,
        "displays": found
            .into_iter()
            .map(|display| serde_json::json!({
                "id": display.id,
                "width": display.width,
                "height": display.height,
            }))
            .collect::<Vec<_>>(),
        "windows": windows
            .into_iter()
            .map(|window| serde_json::json!({
                "id": window.id,
                "title": window.title,
                "application": window.application,
            }))
            .collect::<Vec<_>>(),
    }))
}

/// A escolha da interface virando receita de captura: `{quality, fps, source, audio,
/// muteCalls}`, com `source` como `display:<id>` ou `window:<id>` (ausente é o monitor
/// principal) e `quality` como `720`, `1080`, `1440` ou `2160`.
pub fn capture_config(choice: &serde_json::Value) -> CaptureConfig {
    let source = match choice["source"]
        .as_str()
        .and_then(|text| text.split_once(':'))
    {
        Some(("display", id)) => id.parse().map(CaptureSource::Display).unwrap_or_default(),
        Some(("window", id)) => id.parse().map(CaptureSource::Window).unwrap_or_default(),
        _ => CaptureSource::PrimaryDisplay,
    };

    let quality = match choice["quality"].as_str().unwrap_or_default() {
        "720" => capture::Quality::Hd720,
        "1440" => capture::Quality::Qhd1440,
        "2160" => capture::Quality::Uhd2160,
        _ => capture::Quality::Hd1080,
    };

    CaptureConfig {
        quality,
        source,
        frame_rate: choice["fps"]
            .as_u64()
            .map_or(60, |fps| fps.clamp(1, 60) as u32),
        capture_audio: choice["audio"].as_bool().unwrap_or(true),
        mute_listed_apps: choice["muteCalls"].as_bool().unwrap_or(true),
        ..CaptureConfig::default()
    }
}

/// O caminho de volta do `capture_config`: a receita no ar escrita como o seletor a manda. É
/// o que se guarda para a transmissão voltar sozinha depois de uma atualização.
pub fn choice_of(config: &CaptureConfig) -> serde_json::Value {
    let source = match config.source {
        CaptureSource::Display(id) => format!("display:{id}"),
        CaptureSource::Window(id) => format!("window:{id}"),
        _ => String::new(),
    };
    let quality = match config.quality {
        capture::Quality::Hd720 => "720",
        capture::Quality::Hd1080 => "1080",
        capture::Quality::Qhd1440 => "1440",
        capture::Quality::Uhd2160 => "2160",
    };

    serde_json::json!({
        "source": source,
        "quality": quality,
        "fps": config.frame_rate,
        "audio": config.capture_audio,
        "muteCalls": config.mute_listed_apps,
    })
}

#[derive(Default)]
pub struct ActiveSession(pub tokio::sync::Mutex<Session>);

/// Uma sessão no servidor: um socket e uma chave SRTP para tudo o que sobe. O mediasoup
/// tem um transporte de entrada por peer, então tela, microfone e câmera passam pelo
/// mesmo remetente, cada um com o seu SSRC.
pub struct Session {
    sender: Target,
    key: [u8; 30],
    /// A base dos SSRC desta transmissão. Sorteada por sessão porque o `RtpListener` do
    /// mediasoup é por sala: dois SSRC iguais nela e o segundo a pedir não transmite.
    ssrc_base: u32,
    pub screen: Option<Broadcast>,
    pub voice: Option<Broadcast>,
    pub camera: Option<Broadcast>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            sender: Arc::default(),
            key: PlainSender::generate_key(),
            ssrc_base: PlainSender::random_ssrc_base(),
            screen: None,
            voice: None,
            camera: None,
        }
    }
}

impl Session {
    /// O que o servidor precisa saber antes do primeiro pacote de uma origem, inclusive a
    /// chave que o protege — a mesma para todas: é um transporte só do lado de lá.
    pub fn sfu_offer(&self, source: Source) -> serde_json::Value {
        serde_json::json!({
            "rtpParameters": PlainSender::rtp_parameters(source, self.ssrc_base),
            "srtpParameters": {
                "cryptoSuite": PlainSender::CRYPTO_SUITE,
                "keyBase64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, self.key),
            },
        })
    }

    /// Sorteia uma chave nova para republicar depois que o servidor reiniciou.
    ///
    /// O remetente vai junto: ele numera os pacotes com a chave antiga, e reapontar monta
    /// um `SrtpContext` do zero, com sequenciador aleatório novo. Repetir a chave com o
    /// contador reiniciado repetiria o keystream, e dois trechos cifrados com o mesmo
    /// keystream se abrem um contra o outro.
    pub fn renew_sfu_key(&mut self) {
        self.key = PlainSender::generate_key();
        self.ssrc_base = PlainSender::random_ssrc_base();
        *target(&self.sender) = None;
    }

    /// Aponta a sessão para a porta que o servidor devolveu. Chamar de novo com o mesmo
    /// endereço não faz nada: o remetente é um só, e trocá-lo recomeçaria a numeração.
    pub fn use_sfu(&self, address: &str, server_key: Option<Vec<u8>>) -> anyhow::Result<()> {
        let server = media::resolve(address)?;
        let mut sender = target(&self.sender);

        if sender
            .as_ref()
            .is_some_and(|current| current.server() == server)
        {
            return Ok(());
        }

        *sender = Some(PlainSender::connect(
            server,
            &self.key,
            server_key.as_deref(),
            self.ssrc_base,
        )?);

        Ok(())
    }

    /// O servidor parou de responder com algo subindo — ver `PlainSender::lost_the_server`.
    pub fn lost_the_server(&self) -> bool {
        target(&self.sender)
            .as_ref()
            .is_some_and(|sender| sender.lost_the_server(Instant::now()))
    }

    /// O mesmo `start`, para chamar sem o cadeado da sessão na mão: captura e encoder levam até
    /// segundos para abrir.
    pub fn launcher(&self) -> impl FnOnce(CaptureConfig, Option<Source>, Option<Source>) -> anyhow::Result<Broadcast> + use<> {
        let sender = Arc::clone(&self.sender);

        move |config, video, audio| Broadcast::start(sender, config, video, audio, false)
    }

    /// Liga uma das três origens. `video`/`audio` dizem com que SSRC cada evento sobe.
    pub fn start(
        &self,
        config: CaptureConfig,
        video: Option<Source>,
        audio: Option<Source>,
    ) -> anyhow::Result<Broadcast> {
        Broadcast::start(Arc::clone(&self.sender), config, video, audio, false)
    }

    /// Solta o remetente quando a última origem para: a próxima sessão no servidor pode
    /// cair na mesma porta com outra chave, e um remetente guardado a atravessaria calado.
    /// A chave vai junto: o remetente novo recomeça a numeração, e a mesma chave com a
    /// numeração reiniciada repetiria o keystream.
    pub fn release_if_idle(&mut self) {
        if self.screen.is_none() && self.voice.is_none() && self.camera.is_none() {
            self.renew_sfu_key();
        }
    }

    /// Tudo parado, na saída do app: no Linux cada origem é um `gst-launch`, e a janela
    /// fechar não o mata sozinha.
    pub fn stop_all(&mut self) {
        for mut broadcast in [self.screen.take(), self.voice.take(), self.camera.take()]
            .into_iter()
            .flatten()
        {
            let _ = broadcast.stop();
        }

        self.release_if_idle();
    }
}

/// A escada de uma transmissão: a qualidade escolhida, e abaixo dela 720p e 720p30 — só os
/// degraus que de fato descem.
fn rungs(chosen: (capture::Quality, u32)) -> Vec<(capture::Quality, u32)> {
    let mut rungs = vec![chosen];

    for rung in [(capture::Quality::Hd720, chosen.1), (capture::Quality::Hd720, chosen.1.min(30))] {
        if rungs.last() != Some(&rung) {
            rungs.push(rung);
        }
    }

    rungs
}

/// De quanto em quanto a última imagem se repete com a captura calada: um por segundo, como o
/// WebRTC numa tela parada.
#[cfg(target_os = "windows")]
const STILL_EVERY: Duration = Duration::from_secs(1);

/// A thread que repete a última imagem — ou uma preta, antes da primeira — enquanto a captura
/// não entrega quadro: tela parada, janela minimizada. Sem quadro nenhum o servidor derrubava a
/// transmissão em 30 s (`producerDead`), e quem assistia não tinha como separar a tela parada da
/// travada. É também quem atende, na hora, o quadro-chave pedido por quem entra numa tela parada:
/// antes ele esperava o vigia refazer a captura, até um minuto depois.
#[cfg(target_os = "windows")]
struct StillFrames {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl StillFrames {
    /// `last_frame` é o carimbo e a hora do último quadro de verdade, que a captura atualiza.
    fn start(
        encoding: Arc<Mutex<VideoEncoding>>,
        sfu: Target,
        (video, frame_rate): (Source, f64),
        (last_frame, muted): (Arc<Mutex<(u64, Instant)>>, Arc<AtomicBool>),
    ) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("unkvoid-tela-parada".into())
            .spawn({
                let stop = Arc::clone(&stop);

                move || {
                    let mut still_at = Instant::now();

                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(STILL_EVERY / 4);

                        let (real_ns, real_at) = *last_frame.lock().unwrap_or_else(PoisonError::into_inner);
                        let now = Instant::now();

                        if muted.load(Ordering::Relaxed) || now.duration_since(real_at) < STILL_EVERY {
                            continue;
                        }

                        let feedback = target(&sfu).as_mut().map(PlainSender::read_feedback).unwrap_or_default();
                        let Ok(mut encoding) = encoding.lock() else {
                            continue;
                        };
                        let asked = encoding.keyframes.due(feedback.keyframe, now);

                        encoding.nacked += u64::from(feedback.lost);

                        if !asked && now.duration_since(still_at) < STILL_EVERY {
                            continue;
                        }

                        if asked {
                            encoding.encoder.request_keyframe();
                        }

                        still_at = now;

                        let encoded = encoding.encoder.encode_again(real_ns + now.duration_since(real_at).as_nanos() as u64);

                        if let Ok(frame) = &encoded
                            && frame.keyframe
                        {
                            encoding.keyframes.served(now);
                        }

                        drop(encoding);

                        if let (Ok(frame), Some(sender)) = (encoded, target(&sfu).as_mut()) {
                            let _ = sender.send_frame(video, frame, frame_rate);
                        }
                    }
                }
            })
            .ok()?;

        Some(Self { stop, thread: Some(thread) })
    }
}

#[cfg(target_os = "windows")]
impl Drop for StillFrames {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct Broadcast {
    capturer: PlatformCapturer,
    pub source: CaptureSource,
    /// `"gpu"` ou `"cpu"`: sem encoder na placa a interface avisa que a imagem caiu.
    encoder: &'static str,
    /// Mudo é mandar silêncio: o pipeline continua, e o servidor continua recebendo pacote.
    muted: Arc<AtomicBool>,
    level: Arc<LevelMeter>,
    captured: Arc<AtomicU64>,
    encoded: Arc<AtomicU64>,
    sent: Arc<AtomicU64>,
    encode_errors: Arc<AtomicU64>,
    send_errors: Arc<AtomicU64>,
    send_dropped: Arc<AtomicU64>,
    sent_bytes: Arc<AtomicU64>,
    audio_packets: Arc<AtomicU64>,
    audio_errors: Arc<AtomicU64>,

    /// Microssegundos gastos dentro do callback da captura, somados.
    ///
    /// Codificar e mandar acontecem na thread que a captura chama, então cada
    /// microssegundo aqui é um microssegundo em que o Windows não entrega o quadro
    /// seguinte. Dividido por `captured` dá o custo por quadro, e é o número que diz se
    /// os 60 fps que não aparecem são culpa nossa ou do jogo: a 60 Hz há 16 666 µs por
    /// quadro, e o que passar disso derruba fps sozinho.
    busy_us: Arc<AtomicU64>,

    /// Quantas vezes o servidor pediu um quadro-chave, ou seja, quantas vezes ele viu um
    /// buraco na sequência. É a medida de perda que existe entre nós e ele.
    keyframes: Arc<AtomicU64>,

    /// A taxa que o governador pediu ao encoder, em bits por segundo, e a perda da última
    /// janela medida, em ‰. Escritas uma vez por janela.
    target_bitrate: Arc<AtomicU64>,
    loss_permille: Arc<AtomicU64>,

    /// A receita desta transmissão, guardada para refazer captura e encoder com outra
    /// qualidade sem fechar o producer: o destino é o mesmo, e a sala não vê nada sumir.
    sfu: Target,
    config: CaptureConfig,
    video: Option<Source>,
    audio_source: Option<Source>,
    /// O encoder da placa já falhou nesta transmissão: os refazeres seguintes vão pelo do
    /// processador.
    software: bool,
    /// Quem repete a última imagem quando a captura cala — ver `StillFrames`.
    #[cfg(target_os = "windows")]
    still: Option<StillFrames>,

    /// O encoder e o governador, para o vigia perguntar se a perda pede um degrau.
    encoding: Arc<Mutex<VideoEncoding>>,
    /// A qualidade e o fps que a pessoa escolheu, e quantos degraus abaixo deles a perda
    /// empurrou a transmissão (`step_down`).
    chosen: (capture::Quality, u32),
    steps: usize,
}

impl Broadcast {
    /// Começa a capturar e a codificar. O destino é o da sessão, e entra no `use_sfu`.
    fn start(
        sfu: Target,
        config: CaptureConfig,
        video: Option<Source>,
        audio_source: Option<Source>,
        software: bool,
    ) -> anyhow::Result<Self> {
        let encoder_config = EncoderConfig {
            software,
            ..EncoderConfig::new(config.quality, config.frame_rate, PlatformCapturer::source_size(config.source)?)
        };

        let frame_rate = encoder_config.frame_rate;

        // Cada etapa anuncia que vai começar, não que terminou. As três abaixo mexem com
        // hardware por dentro — no Windows são Media Foundation, Opus e Graphics Capture
        // — e uma delas morrendo leva o processo junto, sem erro e sem pânico. Quem diz
        // onde foi é a última destas linhas que aparecer no arquivo.
        tracing::info!(
            width = encoder_config.width,
            height = encoder_config.height,
            frame_rate,
            bitrate = encoder_config.bitrate,
            "broadcast: abrindo o encoder de vídeo"
        );

        let encoder = PlatformEncoder::new(&encoder_config)?;
        let encoder_kind = if encoder.hardware() { "gpu" } else { "cpu" };

        tracing::info!(encoder = encoder_kind, "broadcast: encoder de vídeo aberto");

        let dropped_so_far = target(&sfu).as_ref().map_or(0, PlainSender::dropped);

        let target_bitrate = Arc::new(AtomicU64::new(u64::from(encoder.bitrate())));
        let loss_permille = Arc::new(AtomicU64::new(0));
        let video_packets = AtomicU64::new(0);

        let encoding = Arc::new(Mutex::new(VideoEncoding {
            governor: BitrateGovernor::from_env(encoder.bitrate()),
            encoder,
            window_started: Instant::now(),
            nacked: 0,
            keyframes: KeyframeGate::default(),
            dropped_before: dropped_so_far,
        }));
        let last_frame = Arc::new(Mutex::new((0_u64, Instant::now())));
        let video_encoding = Arc::clone(&encoding);
        let (recipe_quality, recipe_frame_rate) = (config.quality, config.frame_rate);

        tracing::info!("broadcast: abrindo o encoder de áudio");

        let audio = Mutex::new(AudioEncoder::new(if audio_source == Some(Source::Mic) {
            48_000
        } else {
            96_000
        })?);
        let capture_target = Arc::clone(&sfu);
        let recipe = config.clone();
        let muted = Arc::new(AtomicBool::new(false));
        let muted_callback = Arc::clone(&muted);
        #[cfg(target_os = "windows")]
        let still = video.and_then(|video| {
            StillFrames::start(
                Arc::clone(&encoding),
                Arc::clone(&sfu),
                (video, frame_rate),
                (Arc::clone(&last_frame), Arc::clone(&muted)),
            )
        });
        let level = Arc::new(LevelMeter::default());
        let level_callback = Arc::clone(&level);
        let captured = Arc::new(AtomicU64::new(0));
        let encoded = Arc::new(AtomicU64::new(0));
        let sent = Arc::new(AtomicU64::new(0));
        let encode_errors = Arc::new(AtomicU64::new(0));
        let send_errors = Arc::new(AtomicU64::new(0));
        let send_dropped = Arc::new(AtomicU64::new(dropped_so_far));
        let sent_bytes = Arc::new(AtomicU64::new(0));
        let audio_packets = Arc::new(AtomicU64::new(0));
        let audio_errors = Arc::new(AtomicU64::new(0));
        let busy_us = Arc::new(AtomicU64::new(0));
        let keyframes = Arc::new(AtomicU64::new(0));
        let captured_callback = Arc::clone(&captured);
        let encoded_callback = Arc::clone(&encoded);
        let sent_callback = Arc::clone(&sent);
        let encode_errors_callback = Arc::clone(&encode_errors);
        let send_errors_callback = Arc::clone(&send_errors);
        let send_dropped_callback = Arc::clone(&send_dropped);
        let sent_bytes_callback = Arc::clone(&sent_bytes);
        let audio_packets_callback = Arc::clone(&audio_packets);
        let audio_errors_callback = Arc::clone(&audio_errors);
        let busy_us_callback = Arc::clone(&busy_us);
        let keyframes_callback = Arc::clone(&keyframes);
        let target_bitrate_callback = Arc::clone(&target_bitrate);
        let loss_permille_callback = Arc::clone(&loss_permille);
        let last_frame_callback = Arc::clone(&last_frame);

        tracing::info!(
            source = ?config.source,
            capture_audio = config.capture_audio,
            mute_listed_apps = config.mute_listed_apps,
            "broadcast: abrindo a captura"
        );

        let source = config.source;
        let capturer = PlatformCapturer::start(
            &CaptureConfig {
                frame_rate: frame_rate as u32,
                ..config
            },
            move |event| {
                let muted = muted_callback.load(Ordering::Relaxed);

                let (frame, video_source) = match event {
                    CaptureEvent::Video(frame) => {
                        let Some(video_source) = video else {
                            return;
                        };

                        if muted {
                            return;
                        }

                        captured_callback.fetch_add(1, Ordering::Relaxed);
                        (frame, video_source)
                    }
                    CaptureEvent::Audio(mut block) => {
                        let Some(audio_source) = audio_source else {
                            return;
                        };

                        // Medido ANTES do mudo, e com ele ligado também: na detecção de voz
                        // é a interface que fecha o mic com `set_voice_muted` quando a
                        // pessoa se cala, e só o nível subindo de novo o reabre.
                        level_callback.push(&block.samples);

                        // Mutado sobe silêncio, não nada. Quem marca "silenciar ao entrar"
                        // entra na voz já mutado: sem pacote nenhum o `comedia` do servidor
                        // nunca aprende de onde o mic vem, o relógio de 30 s sem pacote mata
                        // o producer (`producerDead`), e desmutar dava 404. É o que a trilha
                        // desligada do WebRTC faz nos outros sistemas.
                        if muted {
                            block.samples.fill(0.0);
                        }

                        let Ok(mut audio) = audio.lock() else {
                            audio_errors_callback.fetch_add(1, Ordering::Relaxed);
                            return;
                        };

                        let packets = match audio.push(&block) {
                            Ok(packets) => packets,
                            Err(error) => {
                                audio_errors_callback.fetch_add(1, Ordering::Relaxed);
                                tracing::warn!(error = %error, "áudio: bloco recusado");

                                return;
                            }
                        };

                        drop(audio);

                        if let Some(sender) = target(&capture_target).as_mut() {
                            for packet in &packets {
                                match sender.send_audio(audio_source, packet) {
                                    Ok(()) => {
                                        audio_packets_callback.fetch_add(1, Ordering::Relaxed);
                                        sent_bytes_callback
                                            .store(sender.sent_bytes(), Ordering::Relaxed);
                                    }
                                    Err(_) => {
                                        audio_errors_callback.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        }

                        return;
                    }
                };

                let Some(surface) = frame.surface.as_ref() else {
                    return;
                };

                let started = Instant::now();

                let feedback = target(&capture_target)
                    .as_mut()
                    .map(|sender| sender.read_feedback())
                    .unwrap_or_default();

                let encoded = {
                    let Ok(mut encoding) = encoding.lock() else {
                        return;
                    };

                    if feedback.keyframe {
                        keyframes_callback.fetch_add(1, Ordering::Relaxed);
                    }

                    if encoding.keyframes.due(feedback.keyframe, started) {
                        encoding.encoder.request_keyframe();
                    }

                    encoding.nacked += u64::from(feedback.lost);

                    if started.duration_since(encoding.window_started) >= RATE_WINDOW {
                        encoding.close_window(
                            started,
                            video_packets.swap(0, Ordering::Relaxed),
                            send_dropped_callback.load(Ordering::Relaxed),
                        );
                        target_bitrate_callback
                            .store(u64::from(encoding.governor.target()), Ordering::Relaxed);
                        loss_permille_callback.store(
                            u64::from(encoding.governor.loss_permille()),
                            Ordering::Relaxed,
                        );
                    }

                    match encoding.encoder.encode(surface, frame.timestamp_ns) {
                        Ok(encoded) => {
                            encoded_callback.fetch_add(1, Ordering::Relaxed);
                            *last_frame_callback.lock().unwrap_or_else(PoisonError::into_inner) = (frame.timestamp_ns, started);

                            if encoded.keyframe {
                                encoding.keyframes.served(started);
                            }

                            encoded
                        }
                        // `NeedsMoreInput` é a fila do encoder de hardware enchendo, não
                        // defeito. Contar como erro fazia o diagnóstico acusar falha no
                        // começo de toda transmissão, que é justamente quando o encoder
                        // de placa está enchendo a fila dele.
                        Err(media::EncoderError::NeedsMoreInput) => return,
                        // Uma linha, na primeira vez: em nível de depuração ela não era gravada,
                        // e o encoder travado sumia do log de quem transmitiu.
                        Err(error) => {
                            if encode_errors_callback.fetch_add(1, Ordering::Relaxed) == 0 {
                                tracing::warn!(error = %error, "encoder: quadro sem saída (os próximos só contam)");
                            }

                            return;
                        }
                    }
                };

                if let Some(sender) = target(&capture_target).as_mut() {
                    sender.follow_bitrate(target_bitrate_callback.load(Ordering::Relaxed));

                    match sender.send_frame(video_source, encoded, frame_rate) {
                        Ok(packets) => {
                            sent_callback.fetch_add(1, Ordering::Relaxed);
                            video_packets.fetch_add(packets as u64, Ordering::Relaxed);
                            // Lido com o cadeado já na mão: uplink saturado larga pacote
                            // sem devolver erro, e sem este número some do diagnóstico.
                            send_dropped_callback.store(sender.dropped(), Ordering::Relaxed);
                            sent_bytes_callback.store(sender.sent_bytes(), Ordering::Relaxed);
                        }
                        // Uma linha, na primeira vez: a rede que caiu falha sessenta vezes
                        // por segundo, e o contador já conta as outras.
                        Err(error) => {
                            if send_errors_callback.fetch_add(1, Ordering::Relaxed) == 0 {
                                tracing::warn!(error = %error, "transporte: quadro não saiu (as próximas só contam)");
                            }
                        }
                    }
                }

                busy_us_callback.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
            },
        )?;

        Ok(Self {
            capturer,
            source,
            encoder: encoder_kind,
            muted,
            level,
            captured,
            encoded,
            sent,
            encode_errors,
            send_errors,
            send_dropped,
            busy_us,
            keyframes,
            target_bitrate,
            loss_permille,
            sent_bytes,
            audio_packets,
            audio_errors,
            sfu,
            config: recipe,
            video,
            audio_source,
            software,
            #[cfg(target_os = "windows")]
            still,
            encoding: video_encoding,
            chosen: (recipe_quality, recipe_frame_rate),
            steps: 0,
        })
    }

    /// Troca resolução, fps **e a tela** sem fechar o producer: captura e encoder são
    /// refeitos no mesmo destino, e quem assiste só vê a imagem mudar no quadro-chave
    /// seguinte. Recusada a receita nova (placa sem H.264 em 4K, janela que fechou), a
    /// anterior volta e o erro sobe para quem pediu.
    ///
    /// Trocar de monitor por aqui, e não parando e começando de novo, é o que mantém o
    /// mesmo SSRC no ar: o servidor recusa um SSRC repetido na sala, e a transmissão que
    /// parecia só mudar de tela morria.
    ///
    /// ponytail: os contadores recomeçam do zero, então a linha de estatística da
    /// interface pula uma leitura. Guardá-los fora da transmissão seria o passo seguinte.
    pub fn restart(
        &mut self,
        quality: capture::Quality,
        frame_rate: u32,
        source: Option<capture::CaptureSource>,
    ) -> anyhow::Result<()> {
        let previous = self.config.clone();
        let mut wanted = self.config.clone();

        wanted.quality = quality;
        wanted.frame_rate = frame_rate;

        if let Some(source) = source {
            wanted.source = source;
        }

        self.stop()?;

        match Self::start(Arc::clone(&self.sfu), wanted, self.video, self.audio_source, self.software) {
            Ok(fresh) => {
                *self = fresh;

                Ok(())
            }
            Err(error) => {
                tracing::warn!(error = %error, "broadcast: qualidade nova recusada, voltando à anterior");

                *self = Self::start(
                    Arc::clone(&self.sfu),
                    previous,
                    self.video,
                    self.audio_source,
                    self.software,
                )?;

                Err(error)
            }
        }
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    /// Para onde vai o nível do áudio capturado, uns dez por segundo. Vale uma vez por
    /// transmissão, e só a voz chama.
    pub fn on_level(&self, sink: impl Fn(f32) + Send + Sync + 'static) {
        let _ = self.level.sink.set(Box::new(sink));
    }

    pub fn frames(&self) -> u64 {
        self.capturer.frames_captured()
    }

    pub fn counts(&self) -> Counts {
        Counts {
            captured: self.captured.load(Ordering::Relaxed),
            encoded: self.encoded.load(Ordering::Relaxed),
            sent: self.sent.load(Ordering::Relaxed),
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    /// Refaz captura e encoder com a mesma receita, no mesmo producer: é o que o vigia faz
    /// quando uma etapa para de produzir.
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        self.restart(self.config.quality, self.config.frame_rate, None)
    }

    /// A perda continua com a taxa no piso desta qualidade: o caminho não leva nem a menor taxa,
    /// e só um degrau de resolução resolve — o que o WebRTC faz no navegador.
    /// Falso no último degrau: abaixo dele não há o que fazer.
    pub fn starved(&self) -> bool {
        rungs(self.chosen).len() > self.steps + 1 && self.encoding.lock().is_ok_and(|encoding| encoding.governor.starved())
    }

    /// Um minuto limpo no teto de uma qualidade abaixo da escolhida: o degrau de cima cabe.
    pub fn roomy(&self) -> bool {
        self.steps > 0 && self.encoding.lock().is_ok_and(|encoding| encoding.governor.roomy())
    }

    /// Um degrau abaixo: 720p, depois 720p30. Já no último, nada.
    pub fn step_down(&mut self) -> anyhow::Result<()> {
        self.step_to(self.steps + 1)
    }

    /// Um degrau acima, até a qualidade que a pessoa escolheu.
    pub fn step_up(&mut self) -> anyhow::Result<()> {
        self.step_to(self.steps.saturating_sub(1))
    }

    fn step_to(&mut self, steps: usize) -> anyhow::Result<()> {
        let chosen = self.chosen;
        let Some(&(quality, frame_rate)) = rungs(chosen).get(steps).filter(|_| steps != self.steps) else {
            return Ok(());
        };

        tracing::warn!(?quality, frame_rate, "transmissão: a rede pediu outro degrau de qualidade");
        self.restart(quality, frame_rate, None)?;
        self.chosen = chosen;
        self.steps = steps;

        Ok(())
    }

    /// A mesma receita no encoder do processador, em até 720p30: pior, mas no ar.
    pub fn fall_back_to_cpu(&mut self) -> anyhow::Result<()> {
        self.software = true;

        self.refresh()
    }

    pub fn on_hardware(&self) -> bool {
        self.encoder == "gpu"
    }

    /// Os números da transmissão no log, de tempos em tempos: é por eles que se vê, no log de
    /// quem transmitiu, em que etapa a imagem parou.
    pub fn log_numbers(&self) {
        let counts = self.counts();

        tracing::info!(
            captured = counts.captured,
            encoded = counts.encoded,
            sent = counts.sent,
            encode_errors = self.encode_errors.load(Ordering::Relaxed),
            send_errors = self.send_errors.load(Ordering::Relaxed),
            send_dropped = self.send_dropped.load(Ordering::Relaxed),
            keyframes_asked = self.keyframes.load(Ordering::Relaxed),
            target_bitrate = self.target_bitrate.load(Ordering::Relaxed),
            loss_permille = self.loss_permille.load(Ordering::Relaxed),
            capture_error = ?self.capturer.error(),
            "transmissão: números"
        );
    }

    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "active": true,
            "encoder": self.encoder,
            "captured": self.captured.load(Ordering::Relaxed),
            "encoded": self.encoded.load(Ordering::Relaxed),
            "sent": self.sent.load(Ordering::Relaxed),
            "encodeErrors": self.encode_errors.load(Ordering::Relaxed),
            "sendErrors": self.send_errors.load(Ordering::Relaxed),
            "sendDropped": self.send_dropped.load(Ordering::Relaxed),
            "busyUs": self.busy_us.load(Ordering::Relaxed),
            "keyframesAsked": self.keyframes.load(Ordering::Relaxed),
            "targetBitrate": self.target_bitrate.load(Ordering::Relaxed),
            "lossPermille": self.loss_permille.load(Ordering::Relaxed),
            "sentBytes": self.sent_bytes.load(Ordering::Relaxed),
            "audioPackets": self.audio_packets.load(Ordering::Relaxed),
            "captureError": self.capturer.error(),
            "audioErrors": self.audio_errors.load(Ordering::Relaxed),
        })
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        // Antes da captura: imagem repetida por um encoder que está fechando misturaria o fim de
        // uma transmissão com o começo da seguinte, no mesmo SSRC.
        #[cfg(target_os = "windows")]
        drop(self.still.take());

        self.capturer.stop()?;

        Ok(())
    }
}

/// O som que a própria interface captura e empurra para cá — o microfone no macOS, onde o
/// `capture` não o abre e o `AVAudioEngine` traz o cancelamento de eco do sistema. Faz o
/// resto do caminho de um `Broadcast` de áudio: mede, cala, codifica e manda.
pub struct AudioFeed {
    sender: Target,
    source: Source,
    encoder: Mutex<AudioEncoder>,
    muted: AtomicBool,
    level: LevelMeter,
    gate: Mutex<Gate>,
}

/// Como o microfone abre: sozinho quando a pessoa fala, só com a tecla apertada, ou sempre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Abre acima do nível dado, de 0 a 100, e segura aberto por `GATE_TAIL` depois.
    Voice(u8),
    PushToTalk,
    #[default]
    Open,
}

impl InputMode {
    /// `voice`, `ptt` ou `open`, como a preferência guardada (`unkvoid:voice`) os chama.
    pub fn parse(name: &str, sensitivity: u64) -> Self {
        match name {
            "voice" => Self::Voice(sensitivity.clamp(0, 100) as u8),
            "ptt" => Self::PushToTalk,
            _ => Self::Open,
        }
    }
}

/// Quanto o microfone fica aberto depois da última sílaba acima do limiar. Sem isto a voz
/// sairia picotada entre uma palavra e outra.
const GATE_TAIL: Duration = Duration::from_millis(350);

/// O chão da escala do nível: -70 dB é zero, 0 dB é cem.
const LEVEL_FLOOR_DB: f32 = -70.0;

#[derive(Default)]
struct Gate {
    mode: InputMode,
    /// A tecla de falar está apertada.
    talking: bool,
    spoke_at: Option<Instant>,
}

impl Gate {
    fn is_open(&mut self, percent: u8, now: Instant) -> bool {
        match self.mode {
            InputMode::Open => true,
            InputMode::PushToTalk => self.talking,
            InputMode::Voice(threshold) => {
                if percent >= threshold {
                    self.spoke_at = Some(now);
                }

                self.spoke_at
                    .is_some_and(|spoke| now.duration_since(spoke) < GATE_TAIL)
            }
        }
    }
}

/// O nível de um bloco na escala de 0 a 100 que a interface desenha e a sensibilidade usa.
pub fn level_percent(samples: &[f32]) -> u8 {
    let level = rms(samples);
    let decibels = 10.0 * (level * level).max(1e-12).log10();

    ((decibels - LEVEL_FLOOR_DB) * (100.0 / -LEVEL_FLOOR_DB))
        .round()
        .clamp(0.0, 100.0) as u8
}

impl AudioFeed {
    /// PCM `f32` estéreo intercalado a 48 kHz, em pedaços de qualquer tamanho.
    pub fn push(&self, samples: &[f32]) {
        // Medido antes do mudo, pelo mesmo motivo do `Broadcast`: é o nível que reabre o
        // microfone na detecção de voz.
        self.level.push(samples);

        let mut block = capture::AudioChunk {
            sample_rate: 48_000,
            channels: 2,
            samples: samples.to_vec(),
        };

        let open = self
            .gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_open(level_percent(samples), Instant::now());

        // Fechado ou mutado sobe silêncio, não nada: sem pacote o servidor mata o producer em 30 s.
        if !open || self.muted.load(Ordering::Relaxed) {
            block.samples.fill(0.0);
        }

        let packets = match self
            .encoder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(&block)
        {
            Ok(packets) => packets,
            Err(error) => {
                tracing::warn!(error = %error, "áudio: bloco recusado");

                return;
            }
        };

        if let Some(sender) = target(&self.sender).as_mut() {
            for packet in &packets {
                let _ = sender.send_audio(self.source, packet);
            }
        }
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn set_input_mode(&self, mode: InputMode) {
        let mut gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);

        gate.mode = mode;
        gate.spoke_at = None;
    }

    /// A tecla de apertar para falar desceu ou subiu.
    pub fn talk(&self, talking: bool) {
        self.gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .talking = talking;
    }

    pub fn on_level(&self, sink: impl Fn(f32) + Send + Sync + 'static) {
        let _ = self.level.sink.set(Box::new(sink));
    }
}

/// O vídeo que a própria interface captura — a câmera no macOS, pelo `AVCaptureSession`. O
/// quadro chega como o buffer de GPU que o sistema entregou e vai direto para o encoder da
/// placa, sem cópia: o mesmo caminho da tela.
///
/// ponytail: bitrate fixo, sem o governador da tela. A câmera é um cartão de 720p; se a
/// rede apertar, entra o `BitrateGovernor` aqui como no `Broadcast`.
#[cfg(target_os = "macos")]
pub struct VideoFeed {
    sender: Target,
    source: Source,
    encoder: Mutex<PlatformEncoder>,
    frame_rate: f64,
}

#[cfg(target_os = "macos")]
impl VideoFeed {
    pub fn push(&self, surface: &capture::GpuSurface, timestamp_ns: u64) {
        let mut encoder = self.encoder.lock().unwrap_or_else(PoisonError::into_inner);

        let asked = target(&self.sender)
            .as_mut()
            .map(|sender| sender.read_feedback())
            .unwrap_or_default();

        if asked.keyframe {
            encoder.request_keyframe();
        }

        let frame = match encoder.encode(surface, timestamp_ns) {
            Ok(frame) => frame,
            Err(media::EncoderError::NeedsMoreInput) => return,
            Err(error) => {
                tracing::warn!(error = %error, "câmera: quadro recusado pelo encoder");

                return;
            }
        };

        drop(encoder);

        if let Some(sender) = target(&self.sender).as_mut() {
            let _ = sender.send_frame(self.source, frame, self.frame_rate);
        }
    }
}

impl Session {
    /// Abre um `VideoFeed` no remetente desta sessão, para quadros do tamanho dado.
    #[cfg(target_os = "macos")]
    pub fn video_feed(
        &self,
        source: Source,
        size: (u32, u32),
        frame_rate: u32,
    ) -> anyhow::Result<VideoFeed> {
        let config = EncoderConfig::new(capture::Quality::Hd720, frame_rate, size);

        Ok(VideoFeed {
            sender: Arc::clone(&self.sender),
            source,
            frame_rate: config.frame_rate,
            encoder: Mutex::new(PlatformEncoder::new(&config)?),
        })
    }

    /// Abre um `AudioFeed` no remetente desta sessão. Chamar depois do `use_sfu`.
    pub fn feed(&self, source: Source) -> anyhow::Result<AudioFeed> {
        Ok(AudioFeed {
            sender: Arc::clone(&self.sender),
            source,
            encoder: Mutex::new(AudioEncoder::new(48_000)?),
            muted: AtomicBool::new(false),
            level: LevelMeter::default(),
            gate: Mutex::default(),
        })
    }
}

/// Sobe uma origem que só o Linux captura pelo Rust. Nos outros sistemas o webview faz
/// `getUserMedia` e produz pelo WebRTC, e este comando não tem o que fazer.
///
/// Quem chama já tem a sessão trancada; abrir o `gst-launch` é bloqueante, e o
/// `block_in_place` avisa o tokio para não esperar esta thread enquanto isso.
#[cfg(target_os = "linux")]
pub fn start_native(
    session: &Session,
    source: CaptureSource,
    video: Option<Source>,
    audio: Option<Source>,
) -> Result<Broadcast, String> {
    tracing::info!(source = ?source, "abrindo captura nativa");

    tokio::task::block_in_place(|| {
        session.start(
            CaptureConfig {
                source,
                capture_audio: audio.is_some(),
                quality: capture::Quality::Hd720,
                frame_rate: 30,
                ..CaptureConfig::default()
            },
            video,
            audio,
        )
    })
    .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "linux"))]
pub fn start_native(
    _session: &Session,
    _source: CaptureSource,
    _video: Option<Source>,
    _audio: Option<Source>,
) -> Result<Broadcast, String> {
    Err("not supported here: the webview does it".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um segundo de vigia: os contadores andaram `captured`, `encoded` e `sent` quadros.
    fn second(watch: &mut StallWatch, counts: &mut Counts, start: Instant, at: u64, (captured, encoded, sent): (u64, u64, u64)) -> Option<Stall> {
        counts.captured += captured;
        counts.encoded += encoded;
        counts.sent += sent;

        watch.tick(*counts, start + Duration::from_secs(at))
    }

    #[test]
    fn a_flowing_broadcast_is_left_alone() {
        let (start, mut counts) = (Instant::now(), Counts::default());
        let mut watch = StallWatch::new(start);

        for at in 1..=30 {
            assert_eq!(second(&mut watch, &mut counts, start, at, (60, 60, 60)), None, "no segundo {at}");
        }
    }

    #[test]
    fn an_encoder_that_stops_returning_frames_is_restarted() {
        let (start, mut counts) = (Instant::now(), Counts::default());
        let mut watch = StallWatch::new(start);

        for at in 1..=4 {
            second(&mut watch, &mut counts, start, at, (60, 60, 60));
        }

        assert_eq!(second(&mut watch, &mut counts, start, 5, (60, 0, 0)), None);
        assert_eq!(second(&mut watch, &mut counts, start, 6, (60, 0, 0)), Some(Stall::Encoder));
    }

    /// Duas paradas seguidas do encoder sem ele provar que voltou levam ao do processador; um
    /// encoder que volta e trava muito depois começa a conta de novo.
    #[test]
    fn an_encoder_that_stops_twice_in_a_row_keeps_failing() {
        let (start, mut counts) = (Instant::now(), Counts::default());
        let mut watch = StallWatch::new(start);

        second(&mut watch, &mut counts, start, 1, (60, 60, 60));

        assert_eq!(second(&mut watch, &mut counts, start, 3, (60, 0, 0)), Some(Stall::Encoder));
        assert!(!watch.encoder_keeps_failing(), "uma parada é refazer na placa");

        watch.restarted(start + Duration::from_secs(3));
        counts = Counts::default();

        let stalled = (4..=10).find(|&at| second(&mut watch, &mut counts, start, at, (60, 0, 0)) == Some(Stall::Encoder));

        assert!(stalled.is_some() && watch.encoder_keeps_failing(), "a segunda seguida vai para o processador");

        watch.restarted(start + Duration::from_secs(20));
        counts = Counts::default();
        second(&mut watch, &mut counts, start, 21, (120, 120, 120));

        assert!(!watch.encoder_keeps_failing(), "voltou: a conta recomeça");
    }

    #[test]
    fn the_ladder_only_has_rungs_that_go_down() {
        use capture::Quality::{Hd720, Hd1080};

        assert_eq!(rungs((Hd1080, 60)), [(Hd1080, 60), (Hd720, 60), (Hd720, 30)]);
        assert_eq!(rungs((Hd1080, 30)), [(Hd1080, 30), (Hd720, 30)]);
        assert_eq!(rungs((Hd720, 60)), [(Hd720, 60), (Hd720, 30)]);
        assert_eq!(rungs((Hd720, 30)), [(Hd720, 30)], "já no fundo, nenhum degrau");
    }

    #[test]
    fn a_still_screen_is_refreshed_ever_less_often() {
        let (start, mut counts) = (Instant::now(), Counts::default());
        let mut watch = StallWatch::new(start);
        let mut refreshed = Vec::new();

        second(&mut watch, &mut counts, start, 1, (60, 60, 60));

        for at in 2..=40 {
            if second(&mut watch, &mut counts, start, at, (0, 0, 0)) == Some(Stall::Capture) {
                refreshed.push(at);
                watch.restarted(start + Duration::from_secs(at));
                counts = Counts::default();
            }
        }

        assert_eq!(refreshed, [4, 10, 22], "a espera dobra: 3 s, 6 s, 12 s");
    }

    #[test]
    fn a_capture_that_comes_back_waits_the_short_time_again() {
        let (start, mut counts) = (Instant::now(), Counts::default());
        let mut watch = StallWatch::new(start);

        second(&mut watch, &mut counts, start, 1, (60, 60, 60));

        assert_eq!(second(&mut watch, &mut counts, start, 4, (0, 0, 0)), Some(Stall::Capture));

        watch.restarted(start + Duration::from_secs(4));
        counts = Counts::default();

        for at in 5..=8 {
            second(&mut watch, &mut counts, start, at, (60, 60, 60));
        }

        assert_eq!(second(&mut watch, &mut counts, start, 11, (0, 0, 0)), Some(Stall::Capture), "voltou aos 3 s");
    }

    #[test]
    fn frames_that_never_reach_the_network_are_told_once() {
        let (start, mut counts) = (Instant::now(), Counts::default());
        let mut watch = StallWatch::new(start);

        second(&mut watch, &mut counts, start, 1, (60, 60, 60));

        let told: Vec<_> = (2..=8).filter_map(|at| second(&mut watch, &mut counts, start, at, (60, 60, 0))).collect();

        assert_eq!(told, [Stall::Transport]);
    }

    #[test]
    fn keyframe_requests_are_spaced_and_none_is_lost() {
        let mut gate = KeyframeGate::default();
        let start = Instant::now();

        assert!(!gate.due(false, start), "sem pedido, nada sai");
        assert!(gate.due(true, start), "o primeiro pedido sai na hora");
        assert!(!gate.due(true, start + Duration::from_millis(300)), "outro logo depois espera");
        assert!(!gate.due(false, start + Duration::from_secs(1)), "e continua esperando");
        assert!(gate.due(false, start + KEYFRAME_SPACING), "mas sai quando o intervalo acaba");
        assert!(!gate.due(false, start + KEYFRAME_SPACING * 3), "e não sai de novo sem pedido");

        let later = start + KEYFRAME_SPACING * 4;

        gate.served(later);

        assert!(!gate.due(true, later + Duration::from_millis(500)), "o do GOP que acabou de sair já atende");
    }

    /// Pedidos que não param espaçam até o GOP; quinze segundos quietos voltam aos 2 s.
    #[test]
    fn keyframe_requests_that_keep_coming_space_out_up_to_the_gop() {
        let mut gate = KeyframeGate::default();
        let start = Instant::now();
        let at = |millis: u64| start + Duration::from_millis(millis);

        assert!(gate.due(true, at(0)));
        assert!(!gate.due(true, at(500)));
        assert!(gate.due(false, at(2_000)), "o que esperou sai no espaço de 2 s");
        assert!(!gate.due(true, at(2_500)));
        assert!(!gate.due(false, at(5_000)), "pedidos seguidos: o espaço passou a 4 s");
        assert!(gate.due(false, at(6_000)));
        assert!(!gate.due(false, at(30_000)), "sem pedido nada sai");
        assert!(gate.due(true, at(30_000)), "depois de quinze segundos quietos sai na hora");
        assert!(!gate.due(true, at(30_500)));
        assert!(gate.due(false, at(32_000)), "e o espaço voltou a 2 s");
    }

    #[test]
    fn a_recipe_written_as_a_choice_reads_back_the_same() {
        for (source, quality) in [
            (CaptureSource::Window(0x0004_0A2C), capture::Quality::Hd720),
            (CaptureSource::Display(2), capture::Quality::Qhd1440),
            (CaptureSource::PrimaryDisplay, capture::Quality::Uhd2160),
        ] {
            let sent = CaptureConfig {
                quality,
                source,
                frame_rate: 30,
                capture_audio: false,
                mute_listed_apps: false,
                ..CaptureConfig::default()
            };
            let back = capture_config(&choice_of(&sent));

            assert_eq!(format!("{:?}", back.source), format!("{:?}", sent.source));
            assert_eq!(format!("{:?}", back.quality), format!("{:?}", sent.quality));
            assert_eq!((back.frame_rate, back.capture_audio, back.mute_listed_apps), (30, false, false));
        }
    }

    #[test]
    fn the_voice_gate_opens_on_speech_and_holds_through_the_pause_between_words() {
        let started = Instant::now();
        let mut gate = Gate {
            mode: InputMode::Voice(35),
            ..Gate::default()
        };

        assert!(!gate.is_open(10, started), "ruído de fundo não abre");
        assert!(gate.is_open(60, started), "fala abre");
        assert!(
            gate.is_open(5, started + Duration::from_millis(200)),
            "a pausa entre palavras não fecha"
        );
        assert!(
            !gate.is_open(5, started + Duration::from_millis(600)),
            "o silêncio fecha"
        );
    }

    #[test]
    fn push_to_talk_follows_the_key_and_open_is_always_open() {
        let now = Instant::now();
        let mut gate = Gate {
            mode: InputMode::PushToTalk,
            ..Gate::default()
        };

        assert!(!gate.is_open(100, now), "sem a tecla, nem gritando");

        gate.talking = true;

        assert!(gate.is_open(0, now));
        assert!(
            Gate::default().is_open(0, now),
            "o modo padrão é sempre aberto"
        );
    }

    #[test]
    fn the_level_scale_goes_from_minus_seventy_decibels_to_zero() {
        assert_eq!(level_percent(&[0.0; 8]), 0);
        assert_eq!(level_percent(&[1.0, -1.0]), 100);
        assert_eq!(
            level_percent(&[0.0316; 8]),
            57,
            "-30 dB fica a 4/7 da escala"
        );
        assert_eq!(InputMode::parse("voice", 250), InputMode::Voice(100));
    }

    #[test]
    fn rms_is_linear_from_silence_to_full_scale() {
        assert_eq!(rms(&[]), 0.0, "bloco vazio não divide por zero");
        assert_eq!(rms(&[0.0; 1920]), 0.0);

        let square: Vec<f32> = (0..1920)
            .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
            .collect();

        assert!(
            (rms(&square) - 1.0).abs() < 1e-6,
            "onda quadrada no teto é 1"
        );

        let sine: Vec<f32> = (0..1920)
            .map(|index| 0.5 * (index as f32 * std::f32::consts::TAU / 96.0).sin())
            .collect();

        assert!(
            (rms(&sine) - 0.5 / 2.0_f32.sqrt()).abs() < 1e-4,
            "senoide de amplitude 0,5 dá 0,3536: {}",
            rms(&sine)
        );
        assert_eq!(rms(&[4.0, -4.0]), 1.0, "amostra estourada não passa de 1");
    }

    #[test]
    fn the_level_is_the_loudest_block_of_each_window() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let meter = LevelMeter::default();

        meter.push(&[1.0; 4]);

        let sink = Arc::clone(&heard);

        assert!(
            meter
                .sink
                .set(Box::new(move |level| sink.lock().unwrap().push(level)))
                .is_ok()
        );
        assert_eq!(
            *meter.window.lock().unwrap(),
            (0.0, 0),
            "sem ninguém ouvindo o bloco nem é medido"
        );

        for amplitude in [0.0, 0.0, 0.5, 0.0] {
            meter.push(&[amplitude; 4]);
        }

        assert!(
            heard.lock().unwrap().is_empty(),
            "quatro blocos ainda não fecham a janela"
        );

        meter.push(&[0.0; 4]);

        assert_eq!(
            *heard.lock().unwrap(),
            [0.5],
            "a sílaba de um bloco só não some na média"
        );

        for _ in 0..LEVEL_WINDOW_BLOCKS {
            meter.push(&[0.25; 4]);
        }

        assert_eq!(
            *heard.lock().unwrap(),
            [0.5, 0.25],
            "a janela seguinte começa do zero"
        );
    }

    #[test]
    fn a_muted_second_still_feeds_the_server_and_costs_little() {
        let mut encoder = AudioEncoder::new(48_000).expect("encoder");
        let silence = capture::AudioChunk {
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0.0; 1920],
        };
        let packets: Vec<Vec<u8>> = (0..50)
            .flat_map(|_| encoder.push(&silence).expect("push"))
            .collect();
        let bytes: usize = packets.iter().map(Vec::len).sum();

        assert_eq!(
            packets.len(),
            50,
            "um pacote a cada 20 ms: é o que segura o relógio de 30 s do servidor"
        );
        // Sem DTX no `AudioEncoder`, o libopus daqui gasta 3 bytes por pacote de silêncio
        // (150 em 1 s); o teto é folgado para outra versão dele não quebrar o teste.
        assert!(
            bytes * 8 < 48_000 / 4,
            "silêncio custou {bytes} bytes em 1 s"
        );
    }
}
