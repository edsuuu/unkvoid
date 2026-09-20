//! Liga captura, encoder e transporte.
//!
//! O quadro é codificado **uma vez**, na placa de vídeo, e sobe **uma vez** para o
//! servidor, que replica para quantas pessoas estiverem assistindo. São essas duas vezes
//! que fazem transmitir enquanto se joga não custar fps: a CPU não codifica, e o upload
//! não cresce com a plateia.

use std::sync::{
    Arc, Mutex, OnceLock, PoisonError,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer};
use media::{AudioEncoder, BitrateGovernor, EncoderConfig, PlainSender, PlatformEncoder, Source};
use tauri::{Emitter, State};

/// De quanto em quanto tempo o governador da taxa recebe os números. Um segundo junta uns
/// mil pacotes em 1080p60, amostra em que 5% de perda quer dizer alguma coisa. O encoder
/// leva bem mais do que isso para chegar a uma taxa nova; quem espera por ele é a carência
/// do governador, não esta janela.
const RATE_WINDOW: Duration = Duration::from_secs(1);

/// O encoder de vídeo e quem decide a taxa dele, atrás do mesmo cadeado: a decisão é
/// aplicada na thread da captura, a única que toca no encoder, no cadeado que o quadro já
/// tomaria de qualquer jeito.
struct VideoEncoding {
    encoder: PlatformEncoder,
    governor: BitrateGovernor,
    window_started: Instant,
    nacked: u64,
    dropped_before: u64,
}

impl VideoEncoding {
    /// Fecha a janela: entrega os números ao governador e aplica o que ele decidir. Roda
    /// uma vez por `RATE_WINDOW`; por quadro a thread da captura só soma contadores.
    fn close_window(&mut self, now: Instant, packets: u64, dropped_total: u64) {
        // Remetente novo (chave renovada) recomeça a contagem dele do zero.
        let dropped = dropped_total.saturating_sub(self.dropped_before);

        self.dropped_before = dropped_total;
        self.window_started = now;

        let Some(bitrate) = self.governor.observe(packets, std::mem::take(&mut self.nacked), dropped) else {
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

        // O remetente sobrevive à troca de qualidade, e o contador dele também: partir de
        // zero faria a primeira janela ver como perda tudo o que já foi largado antes.
        let dropped_so_far = target(&sfu).as_ref().map_or(0, PlainSender::dropped);

        // O teto é a taxa com que o encoder abriu, não a pedida: no degrau do processador
        // ela é a de 720p30, e governar pela pedida faria a primeira "queda" ser uma subida.
        let target_bitrate = Arc::new(AtomicU64::new(u64::from(encoder.bitrate())));
        let loss_permille = Arc::new(AtomicU64::new(0));
        let video_packets = AtomicU64::new(0);

        let encoding = Mutex::new(VideoEncoding {
            governor: BitrateGovernor::from_env(encoder.bitrate()),
            encoder,
            window_started: Instant::now(),
            nacked: 0,
            dropped_before: dropped_so_far,
        });

        tracing::info!("broadcast: abrindo o encoder de áudio");

        // Voz não precisa da taxa do som do sistema: é uma pessoa falando, não música.
        let audio = Mutex::new(AudioEncoder::new(if audio_source == Some(Source::Mic) { 48_000 } else { 96_000 })?);
        let capture_target = Arc::clone(&sfu);
        let recipe = config.clone();
        let muted = Arc::new(AtomicBool::new(false));
        let muted_callback = Arc::clone(&muted);
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

                // Antes de codificar, e uma vez por quadro: é o único momento em que
                // dá para atender o pedido, e ler o socket aqui custa uma syscall que
                // volta vazia na esmagadora maioria dos quadros.
                let feedback = target(&capture_target)
                    .as_mut()
                    .map(|sender| sender.read_feedback())
                    .unwrap_or_default();

                let encoded = {
                    let Ok(mut encoding) = encoding.lock() else {
                        return;
                    };

                    if feedback.keyframe {
                        encoding.encoder.request_keyframe();
                        keyframes_callback.fetch_add(1, Ordering::Relaxed);
                    }

                    encoding.nacked += u64::from(feedback.lost);

                    if started.duration_since(encoding.window_started) >= RATE_WINDOW {
                        encoding.close_window(
                            started,
                            video_packets.swap(0, Ordering::Relaxed),
                            send_dropped_callback.load(Ordering::Relaxed),
                        );
                        target_bitrate_callback.store(u64::from(encoding.governor.target()), Ordering::Relaxed);
                        loss_permille_callback
                            .store(u64::from(encoding.governor.loss_permille()), Ordering::Relaxed);
                    }

                    match encoding.encoder.encode(surface, frame.timestamp_ns) {
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
        })
    }

    /// Troca resolução e fps sem fechar o producer: captura e encoder são refeitos no
    /// mesmo destino, e quem assiste só vê a imagem mudar de tamanho no quadro-chave
    /// seguinte. Recusada a qualidade nova (placa sem H.264 em 4K, por exemplo), a
    /// anterior volta e o erro sobe para quem pediu.
    ///
    /// ponytail: os contadores recomeçam do zero, então a linha de estatística da
    /// interface pula uma leitura. Guardá-los fora da transmissão seria o passo seguinte.
    pub fn restart(&mut self, quality: capture::Quality, frame_rate: u32) -> anyhow::Result<()> {
        let previous = self.config.clone();
        let mut wanted = self.config.clone();

        wanted.quality = quality;
        wanted.frame_rate = frame_rate;

        self.capturer.stop()?;

        match Self::start(Arc::clone(&self.sfu), wanted, self.video, self.audio_source) {
            Ok(fresh) => {
                *self = fresh;

                Ok(())
            }
            Err(error) => {
                tracing::warn!(error = %error, "broadcast: qualidade nova recusada, voltando à anterior");

                *self = Self::start(Arc::clone(&self.sfu), previous, self.video, self.audio_source)?;

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

/// O microfone padrão do sistema, pelo Rust. Enquanto ele está aberto sai o evento
/// `voice:level` com `{ level }`: o RMS linear de 0 a 1, uns dez por segundo, mutado ou não.
///
/// ponytail: sempre o `@DEFAULT_SOURCE@`; um seletor de microfone traria o `device`.
#[tauri::command]
pub async fn start_voice(app: tauri::AppHandle, state: State<'_, ActiveSession>) -> Result<(), String> {
    let mut session = state.0.lock().await;

    if session.voice.is_some() {
        return Ok(());
    }

    let voice = start_native(&session, CaptureSource::Microphone, None, Some(Source::Mic))?;
    let emit_failed = AtomicBool::new(false);

    voice.on_level(move |level| {
        // Uma linha, na primeira vez: a janela que sumiu falha dez vezes por segundo.
        if let Err(error) = app.emit("voice:level", serde_json::json!({ "level": level }))
            && ! emit_failed.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(error = %error, "voz: o nível do microfone não chegou à interface");
        }
    });

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_is_linear_from_silence_to_full_scale() {
        assert_eq!(rms(&[]), 0.0, "bloco vazio não divide por zero");
        assert_eq!(rms(&[0.0; 1920]), 0.0);

        let square: Vec<f32> = (0..1920).map(|index| if index % 2 == 0 { 1.0 } else { -1.0 }).collect();

        assert!((rms(&square) - 1.0).abs() < 1e-6, "onda quadrada no teto é 1");

        let sine: Vec<f32> =
            (0..1920).map(|index| 0.5 * (index as f32 * std::f32::consts::TAU / 96.0).sin()).collect();

        assert!((rms(&sine) - 0.5 / 2.0_f32.sqrt()).abs() < 1e-4, "senoide de amplitude 0,5 dá 0,3536: {}", rms(&sine));
        assert_eq!(rms(&[4.0, -4.0]), 1.0, "amostra estourada não passa de 1");
    }

    #[test]
    fn the_level_is_the_loudest_block_of_each_window() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let meter = LevelMeter::default();

        meter.push(&[1.0; 4]);

        let sink = Arc::clone(&heard);

        assert!(meter.sink.set(Box::new(move |level| sink.lock().unwrap().push(level))).is_ok());
        assert_eq!(*meter.window.lock().unwrap(), (0.0, 0), "sem ninguém ouvindo o bloco nem é medido");

        for amplitude in [0.0, 0.0, 0.5, 0.0] {
            meter.push(&[amplitude; 4]);
        }

        assert!(heard.lock().unwrap().is_empty(), "quatro blocos ainda não fecham a janela");

        meter.push(&[0.0; 4]);

        assert_eq!(*heard.lock().unwrap(), [0.5], "a sílaba de um bloco só não some na média");

        for _ in 0..LEVEL_WINDOW_BLOCKS {
            meter.push(&[0.25; 4]);
        }

        assert_eq!(*heard.lock().unwrap(), [0.5, 0.25], "a janela seguinte começa do zero");
    }

    #[test]
    fn a_muted_second_still_feeds_the_server_and_costs_little() {
        let mut encoder = AudioEncoder::new(48_000).expect("encoder");
        let silence = capture::AudioChunk { sample_rate: 48_000, channels: 2, samples: vec![0.0; 1920] };
        let packets: Vec<Vec<u8>> = (0..50).flat_map(|_| encoder.push(&silence).expect("push")).collect();
        let bytes: usize = packets.iter().map(Vec::len).sum();

        assert_eq!(packets.len(), 50, "um pacote a cada 20 ms: é o que segura o relógio de 30 s do servidor");
        // Sem DTX no `AudioEncoder`, o libopus daqui gasta 3 bytes por pacote de silêncio
        // (150 em 1 s); o teto é folgado para outra versão dele não quebrar o teste.
        assert!(bytes * 8 < 48_000 / 4, "silêncio custou {bytes} bytes em 1 s");
    }
}
