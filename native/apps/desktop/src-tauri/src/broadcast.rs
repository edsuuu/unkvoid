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

/// O destino, compartilhado entre quem transmite (a thread da captura) e quem o define
/// (o comando `use_sfu`, vindo da interface).
type Target = Arc<Mutex<Option<PlainSender>>>;

pub struct Broadcast {
    capturer: PlatformCapturer,
    sfu: Target,
    sfu_key: [u8; 30],
    captured: Arc<AtomicU64>,
    encoded: Arc<AtomicU64>,
    sent: Arc<AtomicU64>,
    encode_errors: Arc<AtomicU64>,
    send_errors: Arc<AtomicU64>,
    send_dropped: Arc<AtomicU64>,
    audio_packets: Arc<AtomicU64>,
    audio_errors: Arc<AtomicU64>,
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
        let encoder_config = EncoderConfig::new(quality, frame_rate);

        // Quem manda no número é o encoder: ele já limitou o pedido à faixa que aceita, e
        // captura e encoder discordarem faria o vídeo chegar acelerado ou aos trancos.
        let frame_rate = encoder_config.frame_rate;

        // O callback da captura é `Fn`: o encoder guarda estado entre quadros e precisa
        // de mutabilidade interior.
        let encoder = Mutex::new(PlatformEncoder::new(&encoder_config)?);
        let audio = Mutex::new(AudioEncoder::new(96_000)?);
        let sfu: Target = Arc::new(Mutex::new(None));
        let capture_target = Arc::clone(&sfu);
        let captured = Arc::new(AtomicU64::new(0));
        let encoded = Arc::new(AtomicU64::new(0));
        let sent = Arc::new(AtomicU64::new(0));
        let encode_errors = Arc::new(AtomicU64::new(0));
        let send_errors = Arc::new(AtomicU64::new(0));
        let send_dropped = Arc::new(AtomicU64::new(0));
        let audio_packets = Arc::new(AtomicU64::new(0));
        let audio_errors = Arc::new(AtomicU64::new(0));
        let captured_callback = Arc::clone(&captured);
        let encoded_callback = Arc::clone(&encoded);
        let sent_callback = Arc::clone(&sent);
        let encode_errors_callback = Arc::clone(&encode_errors);
        let send_errors_callback = Arc::clone(&send_errors);
        let send_dropped_callback = Arc::clone(&send_dropped);
        let audio_packets_callback = Arc::clone(&audio_packets);
        let audio_errors_callback = Arc::clone(&audio_errors);

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
                        captured_callback.fetch_add(1, Ordering::Relaxed);
                        frame
                    }
                    CaptureEvent::Audio(block) => {
                        let Ok(mut audio) = audio.lock() else {
                            audio_errors_callback.fetch_add(1, Ordering::Relaxed);
                            return;
                        };

                        let packets = match audio.push(&block) {
                            Ok(packets) => packets,
                            Err(_) => {
                                audio_errors_callback.fetch_add(1, Ordering::Relaxed);
                                return;
                            }
                        };

                        drop(audio);

                        if let Ok(mut target) = capture_target.lock()
                            && let Some(sender) = target.as_mut()
                        {
                            for packet in &packets {
                                match sender.send_audio(packet) {
                                    Ok(()) => {
                                        audio_packets_callback.fetch_add(1, Ordering::Relaxed);
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

                let encoded = {
                    let Ok(mut encoder) = encoder.lock() else {
                        return;
                    };

                    match encoder.encode(surface, frame.timestamp_ns) {
                        Ok(encoded) => {
                            encoded_callback.fetch_add(1, Ordering::Relaxed);
                            encoded
                        }
                        Err(_) => {
                            encode_errors_callback.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                    }
                };

                // Enviado aqui mesmo, na thread da captura: mandar UDP é uma syscall, e
                // o socket é não-bloqueante, então o pior caso é perder um pacote em vez
                // de segurar o próximo quadro. Antes cada quadro nascia uma task do
                // tokio, sessenta vezes por segundo, para fazer isto.
                if let Ok(mut target) = capture_target.lock()
                    && let Some(sender) = target.as_mut()
                {
                    match sender.send_frame(&encoded, frame_rate) {
                        Ok(()) => {
                            sent_callback.fetch_add(1, Ordering::Relaxed);
                            // Lido com o cadeado já na mão: uplink saturado larga pacote
                            // sem devolver erro, e sem este número some do diagnóstico.
                            send_dropped_callback.store(sender.dropped(), Ordering::Relaxed);
                        }
                        Err(_) => {
                            send_errors_callback.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            },
        )?;

        Ok(Self {
            capturer,
            sfu,
            sfu_key: PlainSender::generate_key(),
            captured,
            encoded,
            sent,
            encode_errors,
            send_errors,
            send_dropped,
            audio_packets,
            audio_errors,
        })
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

    /// Aponta a transmissão para a porta que o servidor devolveu.
    pub fn use_sfu(&self, address: String) -> anyhow::Result<()> {
        let sender = PlainSender::connect(address.as_str(), &self.sfu_key)?;

        *self
            .sfu
            .lock()
            .map_err(|_| anyhow::anyhow!("broadcast state is poisoned"))? = Some(sender);

        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.capturer.frames_captured()
    }

    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "active": true,
            "captured": self.captured.load(Ordering::Relaxed),
            "encoded": self.encoded.load(Ordering::Relaxed),
            "sent": self.sent.load(Ordering::Relaxed),
            "encodeErrors": self.encode_errors.load(Ordering::Relaxed),
            "sendErrors": self.send_errors.load(Ordering::Relaxed),
            "sendDropped": self.send_dropped.load(Ordering::Relaxed),
            "audioPackets": self.audio_packets.load(Ordering::Relaxed),
            "audioErrors": self.audio_errors.load(Ordering::Relaxed),
        })
    }

    pub fn stop(&mut self) -> anyhow::Result<()> {
        self.capturer.stop()?;

        if let Ok(mut target) = self.sfu.lock() {
            *target = None;
        }

        Ok(())
    }
}
