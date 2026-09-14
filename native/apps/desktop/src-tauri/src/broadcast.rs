//! Liga captura, encoder e transporte.
//!
//! O quadro é codificado **uma vez**, na placa de vídeo, e sobe **uma vez** para o
//! servidor, que replica para quantas pessoas estiverem assistindo. São essas duas vezes
//! que fazem transmitir enquanto se joga não custar fps: a CPU não codifica, e o upload
//! não cresce com a plateia.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use capture::{CaptureConfig, CaptureEvent, CaptureSource, PlatformCapturer, Quality};
use media::{AudioEncoder, EncoderConfig, PlainSender, PlatformEncoder};

/// O que sobrevive a trocar a qualidade no meio da transmissão: o destino e os contadores.
///
/// A captura e o encoder são refeitos, mas o `PlainSender` é o mesmo — mesmo SSRC, mesma
/// numeração, mesmo contexto SRTP —, então o servidor continua vendo o mesmo producer e
/// ninguém na sala perde a transmissão. O encoder novo começa num quadro-chave com a
/// resolução nova, e o decodificador de quem assiste troca de tamanho sozinho.
#[derive(Default)]
struct Shared {
    /// O destino, definido pelo `use_sfu` e lido pela thread da captura.
    sfu: Mutex<Option<PlainSender>>,
    captured: AtomicU64,
    encoded: AtomicU64,
    sent: AtomicU64,
    encode_errors: AtomicU64,
    send_errors: AtomicU64,
    send_dropped: AtomicU64,
    sent_bytes: AtomicU64,
    audio_packets: AtomicU64,
    audio_errors: AtomicU64,

    /// Microssegundos gastos dentro do callback da captura, somados.
    ///
    /// Codificar e mandar acontecem na thread que a captura chama, então cada
    /// microssegundo aqui é um microssegundo em que o Windows não entrega o quadro
    /// seguinte. Dividido por `captured` dá o custo por quadro, e é o número que diz se
    /// os 60 fps que não aparecem são culpa nossa ou do jogo: a 60 Hz há 16 666 µs por
    /// quadro, e o que passar disso derruba fps sozinho.
    busy_us: AtomicU64,

    /// Quantas vezes o servidor pediu um quadro-chave, ou seja, quantas vezes ele viu um
    /// buraco na sequência. É a medida de perda que existe entre nós e ele.
    keyframes: AtomicU64,
}

pub struct Broadcast {
    capturer: PlatformCapturer,
    shared: Arc<Shared>,
    sfu_key: [u8; 30],
    quality: Quality,
    frame_rate: u32,
    source: CaptureSource,
    with_audio: bool,
    mute_calls: bool,
}

impl Broadcast {
    /// Começa a capturar e a codificar. O destino entra depois, no `use_sfu`.
    pub fn start(
        quality: Quality,
        frame_rate: u32,
        source: CaptureSource,
        with_audio: bool,
        mute_calls: bool,
    ) -> anyhow::Result<Self> {
        let shared = Arc::new(Shared::default());
        let capturer = Self::launch(&shared, quality, frame_rate, source, with_audio, mute_calls)?;

        Ok(Self {
            capturer,
            shared,
            sfu_key: PlainSender::generate_key(),
            quality,
            frame_rate,
            source,
            with_audio,
            mute_calls,
        })
    }

    /// Troca resolução e fps sem derrubar a transmissão.
    ///
    /// Para a captura e o encoder e sobe os dois de novo, com a qualidade nova, no mesmo
    /// destino. Se o encoder recusar a qualidade nova (placa sem H.264 em 4K, por
    /// exemplo), volta à anterior e devolve o erro: quem pediu fica sabendo, e a sala
    /// continua assistindo.
    pub fn restart(&mut self, quality: Quality, frame_rate: u32) -> anyhow::Result<()> {
        self.capturer.stop()?;

        match Self::launch(&self.shared, quality, frame_rate, self.source, self.with_audio, self.mute_calls) {
            Ok(capturer) => {
                self.capturer = capturer;
                self.quality = quality;
                self.frame_rate = frame_rate;

                Ok(())
            }
            Err(error) => {
                tracing::warn!(error = %error, "broadcast: qualidade nova recusada, voltando à anterior");

                self.capturer = Self::launch(
                    &self.shared,
                    self.quality,
                    self.frame_rate,
                    self.source,
                    self.with_audio,
                    self.mute_calls,
                )?;

                Err(error)
            }
        }
    }

    /// Abre encoder e captura ligados ao destino de `shared`.
    fn launch(
        shared: &Arc<Shared>,
        quality: Quality,
        frame_rate: u32,
        source: CaptureSource,
        with_audio: bool,
        mute_calls: bool,
    ) -> anyhow::Result<PlatformCapturer> {
        // O tamanho da origem é o que mantém a proporção e impede esticar: 4K pedido num
        // monitor 1080p sai em 1080p. Sem ele, o comportamento de antes (16:9 na largura
        // da qualidade) em vez de uma transmissão que não começa.
        let source_size = PlatformCapturer::source_size(source).unwrap_or_else(|error| {
            tracing::warn!(error = %error, "broadcast: tamanho da origem desconhecido, saída em 16:9");

            (quality.width(), quality.width() * 9 / 16)
        });
        let encoder_config = EncoderConfig::new(quality, frame_rate, source_size);

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
        let encoder = Mutex::new(PlatformEncoder::new(&encoder_config)?);

        tracing::info!("broadcast: abrindo o encoder de áudio");

        let audio = Mutex::new(AudioEncoder::new(96_000)?);
        let shared = Arc::clone(shared);

        tracing::info!(
            source = ?source,
            capture_audio = with_audio,
            mute_listed_apps = mute_calls,
            "broadcast: abrindo a captura"
        );

        let capturer = PlatformCapturer::start(
            &CaptureConfig {
                quality,
                source,
                frame_rate: frame_rate as u32,
                capture_audio: with_audio,
                mute_listed_apps: mute_calls,
                ..CaptureConfig::default()
            },
            move |event| {
                let frame = match event {
                    CaptureEvent::Video(frame) => {
                        shared.captured.fetch_add(1, Ordering::Relaxed);
                        frame
                    }
                    CaptureEvent::Audio(block) => {
                        let Ok(mut audio) = audio.lock() else {
                            shared.audio_errors.fetch_add(1, Ordering::Relaxed);
                            return;
                        };

                        let packets = match audio.push(&block) {
                            Ok(packets) => packets,
                            Err(error) => {
                                shared.audio_errors.fetch_add(1, Ordering::Relaxed);
                                tracing::warn!(error = %error, "áudio: bloco recusado");

                                return;
                            }
                        };

                        drop(audio);

                        if let Ok(mut target) = shared.sfu.lock()
                            && let Some(sender) = target.as_mut()
                        {
                            for packet in &packets {
                                match sender.send_audio(packet) {
                                    Ok(()) => {
                                        shared.audio_packets.fetch_add(1, Ordering::Relaxed);
                                        shared.sent_bytes.store(sender.sent_bytes(), Ordering::Relaxed);
                                    }
                                    Err(_) => {
                                        shared.audio_errors.fetch_add(1, Ordering::Relaxed);
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
                let asked = shared
                    .sfu
                    .lock()
                    .ok()
                    .and_then(|mut target| target.as_mut().map(|sender| sender.keyframe_requested()))
                    .unwrap_or(false);

                let encoded = {
                    let Ok(mut encoder) = encoder.lock() else {
                        return;
                    };

                    if asked {
                        encoder.request_keyframe();
                        shared.keyframes.fetch_add(1, Ordering::Relaxed);
                    }

                    match encoder.encode(surface, frame.timestamp_ns) {
                        Ok(encoded) => {
                            shared.encoded.fetch_add(1, Ordering::Relaxed);
                            encoded
                        }
                        // `NeedsMoreInput` é a fila do encoder de hardware enchendo, não
                        // defeito. Contar como erro fazia o diagnóstico acusar falha no
                        // começo de toda transmissão, que é justamente quando o encoder
                        // de placa está enchendo a fila dele.
                        Err(media::EncoderError::NeedsMoreInput) => return,
                        Err(error) => {
                            shared.encode_errors.fetch_add(1, Ordering::Relaxed);
                            tracing::debug!(error = %error, "encoder: quadro sem saída");

                            return;
                        }
                    }
                };

                // Enviado aqui mesmo, na thread da captura: mandar UDP é uma syscall, e
                // o socket é não-bloqueante, então o pior caso é perder um pacote em vez
                // de segurar o próximo quadro. Antes cada quadro nascia uma task do
                // tokio, sessenta vezes por segundo, para fazer isto.
                if let Ok(mut target) = shared.sfu.lock()
                    && let Some(sender) = target.as_mut()
                {
                    match sender.send_frame(&encoded, frame_rate) {
                        Ok(()) => {
                            shared.sent.fetch_add(1, Ordering::Relaxed);
                            // Lido com o cadeado já na mão: uplink saturado larga pacote
                            // sem devolver erro, e sem este número some do diagnóstico.
                            shared.send_dropped.store(sender.dropped(), Ordering::Relaxed);
                            shared.sent_bytes.store(sender.sent_bytes(), Ordering::Relaxed);
                        }
                        Err(error) => {
                            shared.send_errors.fetch_add(1, Ordering::Relaxed);
                            tracing::warn!(error = %error, "transporte: quadro não saiu");
                        }
                    }
                }

                shared.busy_us.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
            },
        )?;

        Ok(capturer)
    }

    /// O que o servidor precisa saber antes do primeiro pacote, inclusive a chave que o
    /// protege. A chave nasce com a transmissão e não muda: é o mesmo contexto que
    /// numera os pacotes.
    pub fn sfu_offer(&self, kind: &str) -> serde_json::Value {
        serde_json::json!({
            "rtpParameters": PlainSender::rtp_parameters(kind),
            "srtpParameters": {
                "cryptoSuite": PlainSender::CRYPTO_SUITE,
                "keyBase64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, self.sfu_key),
            },
        })
    }

    /// Sorteia uma chave nova para republicar depois que o servidor reiniciou.
    ///
    /// Reapontar o destino monta um `SrtpContext` do zero, com sequenciador aleatório
    /// novo. Repetir a chave com o contador reiniciado repetiria o keystream, e dois
    /// trechos cifrados com o mesmo keystream se abrem um contra o outro.
    pub fn renew_sfu_key(&mut self) {
        self.sfu_key = PlainSender::generate_key();
    }

    /// Aponta a transmissão para a porta que o servidor devolveu.
    pub fn use_sfu(&self, address: String, server_key: Option<Vec<u8>>) -> anyhow::Result<()> {
        let sender = PlainSender::connect(address.as_str(), &self.sfu_key, server_key.as_deref())?;

        *self
            .shared
            .sfu
            .lock()
            .map_err(|_| anyhow::anyhow!("broadcast state is poisoned"))? = Some(sender);

        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.capturer.frames_captured()
    }

    pub fn stats(&self) -> serde_json::Value {
        let shared = &self.shared;

        serde_json::json!({
            "active": true,
            "captured": shared.captured.load(Ordering::Relaxed),
            "encoded": shared.encoded.load(Ordering::Relaxed),
            "sent": shared.sent.load(Ordering::Relaxed),
            "encodeErrors": shared.encode_errors.load(Ordering::Relaxed),
            "sendErrors": shared.send_errors.load(Ordering::Relaxed),
            "sendDropped": shared.send_dropped.load(Ordering::Relaxed),
            "busyUs": shared.busy_us.load(Ordering::Relaxed),
            "keyframesAsked": shared.keyframes.load(Ordering::Relaxed),
            "sentBytes": shared.sent_bytes.load(Ordering::Relaxed),
            "audioPackets": shared.audio_packets.load(Ordering::Relaxed),
            "captureError": self.capturer.error(),
            "audioErrors": shared.audio_errors.load(Ordering::Relaxed),
        })
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.capturer.stop()?;

        if let Ok(mut target) = self.shared.sfu.lock() {
            *target = None;
        }

        Ok(())
    }
}
