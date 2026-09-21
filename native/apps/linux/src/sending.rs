//! Transmitir: captura, empacota e sobe.
//!
//! No Linux quem comprime é o GStreamer do lado da captura — `nvh264enc`/`vah264enc` quando
//! há placa, `x264enc` quando não há — e o que chega aqui já é H.264 pronto. Não há segundo
//! encoder: o quadro é comprimido uma vez e sobe uma vez, para o servidor, que replica.
//!
//! Tela, som da tela, microfone e câmera passam pelo MESMO socket e pela mesma chave (o
//! mediasoup tem um transporte de entrada por peer); o que os separa é o SSRC de cada
//! origem.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::Result;
use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer};
use media::{AudioEncoder, EncodedFrame, PlainSender, Source};

/// O destino, compartilhado entre a thread da captura e quem o aponta (o `producePlain`).
type Target = Arc<Mutex<Option<PlainSender>>>;

/// Uma origem no ar: o que a captura abriu e o que o servidor abriu para ela.
struct Live {
    capturer: PlatformCapturer,
    /// Mudo é mandar silêncio, não nada: sem pacote o relógio de 30 s do SFU mata o
    /// producer, e desmutar daria 404.
    muted: Arc<AtomicBool>,
    /// Os producers do servidor desta origem (a tela leva o som dela junto).
    producers: Vec<String>,
    /// A receita, para republicar sem a interface ter de lembrar dela.
    config: CaptureConfig,
}

/// Uma sessão no servidor: um socket, uma chave, e as origens que estão subindo por eles.
pub struct Sending {
    sender: Target,
    key: [u8; 30],
    live: HashMap<Source, Live>,
    /// Erros de envio somados. Log por quadro é proibido: a primeira falha sai no log e o
    /// resto vira número.
    errors: Arc<AtomicU64>,
}

impl Default for Sending {
    fn default() -> Self {
        Self {
            sender: Target::default(),
            key: PlainSender::generate_key(),
            live: HashMap::new(),
            errors: Arc::default(),
        }
    }
}

impl Sending {
    /// O que o servidor precisa saber antes do primeiro pacote de uma origem, inclusive a
    /// chave que o protege — a mesma para todas: é um transporte só do lado de lá.
    pub fn offer(&self, source: Source) -> serde_json::Value {
        serde_json::json!({
            "rtpParameters": PlainSender::rtp_parameters(source),
            "srtpParameters": {
                "cryptoSuite": PlainSender::CRYPTO_SUITE,
                "keyBase64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, self.key),
            },
        })
    }

    /// Aponta a sessão para a porta que o `producePlain` devolveu. Repetir o mesmo endereço
    /// não faz nada: o remetente é um só, e trocá-lo recomeçaria a numeração dos pacotes.
    pub fn use_sfu(&self, address: &str, server_key: Option<Vec<u8>>) -> Result<()> {
        let server = media::resolve(address)?;
        let mut sender = target(&self.sender);

        if sender.as_ref().is_some_and(|current| current.server() == server) {
            return Ok(());
        }

        *sender = Some(PlainSender::connect(server, &self.key, server_key.as_deref())?);

        Ok(())
    }

    pub fn is_live(&self, source: Source) -> bool {
        self.live.contains_key(&source)
    }

    pub fn live_sources(&self) -> Vec<Source> {
        self.live.keys().copied().collect()
    }

    pub fn config_of(&self, source: Source) -> Option<CaptureConfig> {
        self.live.get(&source).map(|live| live.config.clone())
    }

    /// Liga uma origem. `video`/`audio` dizem com que SSRC cada evento sobe: a tela sobe
    /// pelos dois (imagem e som do sistema), o microfone só pelo de áudio.
    pub fn start(
        &mut self,
        key: Source,
        config: CaptureConfig,
        video: Option<Source>,
        audio: Option<Source>,
        producers: Vec<String>,
    ) -> Result<()> {
        let sender = Arc::clone(&self.sender);
        let errors = Arc::clone(&self.errors);
        let muted = Arc::new(AtomicBool::new(false));
        let muted_capture = Arc::clone(&muted);
        let encoder = Mutex::new(AudioEncoder::new(if audio == Some(Source::Mic) {
            48_000
        } else {
            96_000
        })?);
        let frame_rate = f64::from(config.frame_rate.clamp(1, 60));

        tracing::info!(source = ?config.source, ?video, ?audio, "transmissão: abrindo a captura");

        let capturer = PlatformCapturer::start(&config, move |event| {
            let muted = muted_capture.load(Ordering::Relaxed);

            match event {
                CaptureEvent::Video(frame) => {
                    let (Some(video), Some(surface)) = (video, frame.surface.as_ref()) else {
                        return;
                    };

                    if muted {
                        return;
                    }

                    let encoded = EncodedFrame {
                        data: surface.data.clone(),
                        keyframe: surface.keyframe,
                        timestamp_ns: frame.timestamp_ns,
                    };

                    let mut slot = target(&sender);
                    let Some(plain) = slot.as_mut() else {
                        return;
                    };

                    // Lido uma vez por quadro: é o único momento em que dá para esvaziar o
                    // caminho de volta. O que ele pede não tem como ser atendido aqui — o
                    // encoder é o `gst-launch` filho — mas não lê-lo encheria o socket.
                    plain.read_feedback();
                    count(&errors, plain.send_frame(video, encoded, frame_rate).err());
                }
                CaptureEvent::Audio(mut block) => {
                    let Some(audio) = audio else {
                        return;
                    };

                    // Mutado sobe silêncio: sem pacote nenhum o `comedia` do servidor nunca
                    // aprende de onde o microfone vem.
                    if muted {
                        block.samples.fill(0.0);
                    }

                    let Ok(mut encoder) = encoder.lock() else {
                        return;
                    };

                    let packets = match encoder.push(&block) {
                        Ok(packets) => packets,
                        Err(failure) => {
                            count(&errors, Some(anyhow::anyhow!("{failure}")));

                            return;
                        }
                    };

                    drop(encoder);

                    if let Some(plain) = target(&sender).as_mut() {
                        for packet in &packets {
                            count(&errors, plain.send_audio(audio, packet).err());
                        }
                    }
                }
            }
        })?;

        self.live.insert(key, Live { capturer, muted, producers, config });

        Ok(())
    }

    /// Reabre a captura de uma origem **sem** mexer no que o servidor tem: o producer, o
    /// socket e a chave continuam os mesmos. É como o microfone passa a ouvir outro
    /// aparelho sem a sala ver ninguém entrar e sair.
    pub fn restart(&mut self, key: Source, video: Option<Source>, audio: Option<Source>) -> Result<()> {
        let Some(live) = self.live.remove(&key) else {
            return Ok(());
        };

        let (config, producers) = (live.config.clone(), live.producers.clone());
        let muted = live.muted.load(Ordering::Relaxed);

        // O `gst-launch` filho morre com o capturador; sem isto o aparelho velho continuaria
        // aberto e o novo não abriria.
        drop(live);

        self.start(key, config, video, audio, producers)?;
        self.set_muted(key, muted);

        Ok(())
    }

    /// Para uma origem e devolve os producers que o servidor ainda tem abertos dela.
    pub fn stop(&mut self, key: Source) -> Vec<String> {
        let Some(mut live) = self.live.remove(&key) else {
            return Vec::new();
        };

        let _ = live.capturer.stop();

        // Nada mais subindo: o remetente sai e a chave também. A próxima sessão no servidor
        // pode cair na mesma porta com outra chave, e um remetente guardado a atravessaria
        // calado; a mesma chave com a numeração reiniciada repetiria o keystream.
        if self.live.is_empty() {
            self.renew_key();
        }

        live.producers
    }

    pub fn set_muted(&self, key: Source, muted: bool) {
        if let Some(live) = self.live.get(&key) {
            live.muted.store(muted, Ordering::Relaxed);
        }
    }

    /// O producer daquela origem no servidor. O primeiro: a tela publica imagem e som, e
    /// quem pausa é sempre o primeiro (o vídeo da tela, o áudio do microfone).
    pub fn producer_of(&self, key: Source) -> Option<String> {
        self.live.get(&key)?.producers.first().cloned()
    }

    pub fn is_muted(&self, key: Source) -> bool {
        self.live.get(&key).is_some_and(|live| live.muted.load(Ordering::Relaxed))
    }

    /// Sorteia uma chave nova e larga o remetente. Obrigatório antes de republicar: o
    /// remetente numera os pacotes com a chave antiga, e repetir a chave com o contador
    /// reiniciado repetiria o keystream.
    pub fn renew_key(&mut self) {
        self.key = PlainSender::generate_key();
        *target(&self.sender) = None;
    }

    /// Tudo parado: cada origem é um `gst-launch` filho, e a janela fechar não o mata.
    pub fn stop_all(&mut self) -> Vec<String> {
        let sources = self.live_sources();

        sources.into_iter().flat_map(|source| self.stop(source)).collect()
    }
}

impl Drop for Sending {
    fn drop(&mut self) {
        self.stop_all();
    }
}

/// O que a interface escolhe transmitir de cada origem.
pub fn screen_config(quality: capture::Quality, frame_rate: u32, with_audio: bool) -> CaptureConfig {
    CaptureConfig {
        quality,
        source: CaptureSource::PrimaryDisplay,
        frame_rate,
        capture_audio: with_audio,
        mute_listed_apps: true,
        show_cursor: true,
    }
}

pub fn microphone_config() -> CaptureConfig {
    CaptureConfig {
        source: CaptureSource::Microphone,
        capture_audio: true,
        ..CaptureConfig::default()
    }
}

pub fn camera_config(index: u32) -> CaptureConfig {
    CaptureConfig {
        source: CaptureSource::Camera(index),
        capture_audio: false,
        ..CaptureConfig::default()
    }
}

/// Um `Mutex` envenenado aqui é uma thread de captura que caiu. Continuar transmitindo o
/// que dá é melhor do que derrubar o app de quem está na sala.
fn target(sfu: &Target) -> MutexGuard<'_, Option<PlainSender>> {
    sfu.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A primeira falha vira log; as outras, número. A rede que caiu falha sessenta vezes por
/// segundo, e sessenta linhas por segundo não ajudam ninguém.
fn count(errors: &Arc<AtomicU64>, failure: Option<impl std::fmt::Display>) {
    let Some(failure) = failure else {
        return;
    };

    if errors.fetch_add(1, Ordering::Relaxed) == 0 {
        tracing::warn!(error = %failure, "transmissão: não saiu (as próximas só contam)");
    }
}
