//! Liga captura, encoder e transporte.
//!
//! O quadro é codificado **uma vez**, na placa de vídeo, e sobe **uma vez** para o
//! servidor, que replica para quantas pessoas estiverem assistindo. São essas duas vezes
//! que fazem transmitir enquanto se joga não custar fps: a CPU não codifica, e o upload
//! não cresce com a plateia.

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer};
use media::{AudioEncoder, EncoderConfig, PlainSender, PlatformEncoder, Source};
use tauri::State;

/// O destino, compartilhado entre quem transmite (a thread da captura) e quem o define
/// (o comando `use_sfu`, vindo da interface).
type Target = Arc<Mutex<Option<PlainSender>>>;

/// O remetente é um `Option`: uma thread que morreu com o cadeado na mão não deixa
/// estado pela metade, então o veneno é ignorado em vez de derrubar a transmissão.
fn target(sfu: &Target) -> std::sync::MutexGuard<'_, Option<PlainSender>> {
    sfu.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub struct ActiveSession(pub tokio::sync::Mutex<Session>);

/// Uma sessão no servidor: um socket e uma chave SRTP para tudo o que sobe. O mediasoup
/// tem um transporte de entrada por peer, então tela, microfone e câmera passam pelo
/// mesmo remetente, cada um com o seu SSRC.
pub struct Session {
    sender: Target,
    key: [u8; 30],
    pub screen: Option<Broadcast>,
    pub voice: Option<Broadcast>,
    pub camera: Option<Broadcast>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            sender: Arc::default(),
            key: PlainSender::generate_key(),
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
            "rtpParameters": PlainSender::rtp_parameters(source),
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
        *target(&self.sender) = None;
    }

    /// Aponta a sessão para a porta que o servidor devolveu. Chamar de novo com o mesmo
    /// endereço não faz nada: o remetente é um só, e trocá-lo recomeçaria a numeração.
    pub fn use_sfu(&self, address: &str, server_key: Option<Vec<u8>>) -> anyhow::Result<()> {
        // Resolver o nome pode ir ao DNS; a thread da captura não espera por isso.
        let server = media::resolve(address)?;
        let mut sender = target(&self.sender);

        if sender.as_ref().is_some_and(|current| current.server() == server) {
            return Ok(());
        }

        *sender = Some(PlainSender::connect(server, &self.key, server_key.as_deref())?);

        Ok(())
    }

    /// Liga uma das três origens. `video`/`audio` dizem com que SSRC cada evento sobe.
    pub fn start(
        &self,
        config: CaptureConfig,
        video: Option<Source>,
        audio: Option<Source>,
    ) -> anyhow::Result<Broadcast> {
        Broadcast::start(Arc::clone(&self.sender), config, video, audio)
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
        for mut broadcast in [self.screen.take(), self.voice.take(), self.camera.take()].into_iter().flatten() {
            let _ = broadcast.stop();
        }

        self.release_if_idle();
    }
}

pub struct Broadcast {
    capturer: PlatformCapturer,
    pub source: CaptureSource,
    /// `"gpu"` ou `"cpu"`: sem encoder na placa a interface avisa que a imagem caiu.
    encoder: &'static str,
    /// Mudo é não mandar: o pipeline continua, o servidor só para de receber.
    muted: Arc<AtomicBool>,
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
}

impl Broadcast {
    /// Começa a capturar e a codificar. O destino é o da sessão, e entra no `use_sfu`.
    fn start(
        sfu: Target,
        config: CaptureConfig,
        video: Option<Source>,
        audio_source: Option<Source>,
    ) -> anyhow::Result<Self> {
        let encoder_config =
            EncoderConfig::new(config.quality, config.frame_rate, PlatformCapturer::source_size(config.source)?);

        // Quem manda no número é o encoder: ele já limitou o pedido à faixa que aceita, e
        // captura e encoder discordarem faria o vídeo chegar acelerado ou aos trancos.
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

        // O callback da captura é `Fn`: o encoder guarda estado entre quadros e precisa
        // de mutabilidade interior.
        let encoder = PlatformEncoder::new(&encoder_config)?;
        let encoder_kind = if encoder.hardware() { "gpu" } else { "cpu" };

        tracing::info!(encoder = encoder_kind, "broadcast: encoder de vídeo aberto");

        let encoder = Mutex::new(encoder);

        tracing::info!("broadcast: abrindo o encoder de áudio");

        // Voz não precisa da taxa do som do sistema: é uma pessoa falando, não música.
        let audio = Mutex::new(AudioEncoder::new(if audio_source == Some(Source::Mic) { 48_000 } else { 96_000 })?);
        let capture_target = sfu;
        let muted = Arc::new(AtomicBool::new(false));
        let muted_callback = Arc::clone(&muted);
        let captured = Arc::new(AtomicU64::new(0));
        let encoded = Arc::new(AtomicU64::new(0));
        let sent = Arc::new(AtomicU64::new(0));
        let encode_errors = Arc::new(AtomicU64::new(0));
        let send_errors = Arc::new(AtomicU64::new(0));
        let send_dropped = Arc::new(AtomicU64::new(0));
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

        tracing::info!(
            source = ?config.source,
            capture_audio = config.capture_audio,
            mute_listed_apps = config.mute_listed_apps,
            "broadcast: abrindo a captura"
        );

        let source = config.source;
        let capturer = PlatformCapturer::start(
            &CaptureConfig { frame_rate: frame_rate as u32, ..config },
            move |event| {
                if muted_callback.load(Ordering::Relaxed) {
                    return;
                }

                let (frame, video_source) = match event {
                    CaptureEvent::Video(frame) => {
                        let Some(video_source) = video else {
                            return;
                        };

                        captured_callback.fetch_add(1, Ordering::Relaxed);
                        (frame, video_source)
                    }
                    CaptureEvent::Audio(block) => {
                        let Some(audio_source) = audio_source else {
                            return;
                        };

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

                let started = std::time::Instant::now();

                // Antes de codificar, e uma vez por quadro: é o único momento em que
                // dá para atender o pedido, e ler o socket aqui custa uma syscall que
                // volta vazia na esmagadora maioria dos quadros.
                let asked = target(&capture_target)
                    .as_mut()
                    .is_some_and(|sender| sender.keyframe_requested());

                let encoded = {
                    let Ok(mut encoder) = encoder.lock() else {
                        return;
                    };

                    if asked {
                        encoder.request_keyframe();
                        keyframes_callback.fetch_add(1, Ordering::Relaxed);
                    }

                    match encoder.encode(surface, frame.timestamp_ns) {
                        Ok(encoded) => {
                            encoded_callback.fetch_add(1, Ordering::Relaxed);
                            encoded
                        }
                        // `NeedsMoreInput` é a fila do encoder de hardware enchendo, não
                        // defeito. Contar como erro fazia o diagnóstico acusar falha no
                        // começo de toda transmissão, que é justamente quando o encoder
                        // de placa está enchendo a fila dele.
                        Err(media::EncoderError::NeedsMoreInput) => return,
                        Err(error) => {
                            encode_errors_callback.fetch_add(1, Ordering::Relaxed);
                            tracing::debug!(error = %error, "encoder: quadro sem saída");

                            return;
                        }
                    }
                };

                // Enviado aqui mesmo, na thread da captura: mandar UDP é uma syscall, e
                // o socket é não-bloqueante, então o pior caso é perder um pacote em vez
                // de segurar o próximo quadro. Antes cada quadro nascia uma task do
                // tokio, sessenta vezes por segundo, para fazer isto.
                if let Some(sender) = target(&capture_target).as_mut() {
                    match sender.send_frame(video_source, encoded, frame_rate) {
                        Ok(()) => {
                            sent_callback.fetch_add(1, Ordering::Relaxed);
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
            captured,
            encoded,
            sent,
            encode_errors,
            send_errors,
            send_dropped,
            busy_us,
            keyframes,
            sent_bytes,
            audio_packets,
            audio_errors,
        })
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn frames(&self) -> u64 {
        self.capturer.frames_captured()
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
            "sentBytes": self.sent_bytes.load(Ordering::Relaxed),
            "audioPackets": self.audio_packets.load(Ordering::Relaxed),
            "captureError": self.capturer.error(),
            "audioErrors": self.audio_errors.load(Ordering::Relaxed),
        })
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.capturer.stop()?;

        Ok(())
    }
}

/// Sobe uma origem que só o Linux captura pelo Rust. Nos outros sistemas o webview faz
/// `getUserMedia` e produz pelo WebRTC, e este comando não tem o que fazer.
///
/// Quem chama já tem a sessão trancada; abrir o `gst-launch` é bloqueante, e o
/// `block_in_place` avisa o tokio para não esperar esta thread enquanto isso.
#[cfg(target_os = "linux")]
fn start_native(
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
fn start_native(
    _session: &Session,
    _source: CaptureSource,
    _video: Option<Source>,
    _audio: Option<Source>,
) -> Result<Broadcast, String> {
    Err("not supported here: the webview does it".into())
}

/// O microfone padrão do sistema, pelo Rust.
///
/// ponytail: sempre o `@DEFAULT_SOURCE@`; um seletor de microfone traria o `device`.
#[tauri::command]
pub async fn start_voice(state: State<'_, ActiveSession>) -> Result<(), String> {
    let mut session = state.0.lock().await;

    if session.voice.is_some() {
        return Ok(());
    }

    let voice = start_native(&session, CaptureSource::Microphone, None, Some(Source::Mic))?;

    session.voice = Some(voice);

    Ok(())
}

#[tauri::command]
pub async fn stop_voice(state: State<'_, ActiveSession>) -> Result<(), String> {
    let mut session = state.0.lock().await;

    if let Some(mut voice) = session.voice.take() {
        tokio::task::block_in_place(|| voice.stop()).map_err(|error| error.to_string())?;
    }

    session.release_if_idle();

    Ok(())
}

#[tauri::command]
pub async fn set_voice_muted(state: State<'_, ActiveSession>, muted: bool) -> Result<(), String> {
    if let Some(voice) = state.0.lock().await.voice.as_ref() {
        voice.set_muted(muted);
    }

    Ok(())
}

/// O índice de um `id` que `list_cameras` devolveu (`/dev/video<n>`).
pub fn camera_index(device: &str) -> Result<u32, String> {
    device
        .trim_start_matches("/dev/video")
        .parse()
        .map_err(|_| format!("câmera desconhecida: {device}"))
}

/// A câmera, pelo Rust. Pedir outra com uma já ligada troca: a que estava para antes.
#[tauri::command]
pub async fn start_camera(state: State<'_, ActiveSession>, device: String) -> Result<(), String> {
    let source = CaptureSource::Camera(camera_index(&device)?);
    let mut session = state.0.lock().await;

    match session.camera.take() {
        Some(camera) if camera.source == source => {
            session.camera = Some(camera);

            return Ok(());
        }
        Some(mut camera) => tokio::task::block_in_place(|| camera.stop()).map_err(|error| error.to_string())?,
        None => {}
    }

    let camera = start_native(&session, source, Some(Source::Camera), None)?;

    session.camera = Some(camera);

    Ok(())
}

#[tauri::command]
pub async fn stop_camera(state: State<'_, ActiveSession>) -> Result<(), String> {
    let mut session = state.0.lock().await;

    if let Some(mut camera) = session.camera.take() {
        tokio::task::block_in_place(|| camera.stop()).map_err(|error| error.to_string())?;
    }

    session.release_if_idle();

    Ok(())
}
