//! Liga captura, encoder e transporte.
//!
//! O quadro sai da GPU, vai para o encoder por hardware e de lá direto para o RTP —
//! sem passar pela CPU no meio. A sinalização fica na interface, que já sabe falar
//! com o SFU: aqui só entram SDP e candidatos prontos.

use std::sync::Arc;

use capture::{CaptureConfig, CaptureEvent, PlatformCapturer, Quality};
use media::{EncoderConfig, PeerLink, PlatformEncoder, Signal};
use tokio::sync::{Mutex, mpsc};

pub struct Broadcast {
    capturer: PlatformCapturer,
    peer: Arc<Mutex<PeerLink>>,
}

impl Broadcast {
    /// Sobe tudo e devolve a oferta SDP mais o canal de candidatos ICE, que a
    /// interface repassa pelo SFU.
    pub async fn start(
        quality: Quality,
        ice_servers: Vec<String>,
    ) -> anyhow::Result<(Self, String, mpsc::Receiver<Signal>)> {
        let encoder_config = EncoderConfig::for_quality(quality);
        let (peer, sinais) = PeerLink::connect(ice_servers, encoder_config.frame_rate).await?;
        let peer = Arc::new(Mutex::new(peer));

        let oferta = peer.lock().await.create_offer().await?;

        // O callback da captura é Fn, não FnMut: o encoder guarda estado entre
        // quadros, então precisa de mutabilidade interior.
        let encoder = std::sync::Mutex::new(PlatformEncoder::new(&encoder_config)?);
        let envio = Arc::clone(&peer);
        let runtime = tokio::runtime::Handle::current();

        let capturer = PlatformCapturer::start(
            &CaptureConfig {
                quality,
                ..CaptureConfig::default()
            },
            move |evento| {
                let CaptureEvent::Video(quadro) = evento else {
                    return;
                };

                let Some(surface) = quadro.surface.as_ref() else {
                    return;
                };

                // Codificar aqui, na thread da captura, evita copiar o buffer da GPU
                // para outra thread só para comprimir.
                let Ok(mut encoder) = encoder.lock() else {
                    return;
                };

                let Ok(codificado) = encoder.encode(surface, quadro.timestamp_ns) else {
                    return;
                };

                drop(encoder);

                let peer = Arc::clone(&envio);

                runtime.spawn(async move {
                    let _ = peer.lock().await.send_frame(&codificado).await;
                });
            },
        )?;

        Ok((Self { capturer, peer }, oferta, sinais))
    }

    pub async fn accept_answer(&self, sdp: String) -> anyhow::Result<()> {
        self.peer.lock().await.accept_answer(sdp).await
    }

    pub async fn add_candidate(&self, json: String) -> anyhow::Result<()> {
        self.peer.lock().await.add_candidate(json).await
    }

    pub fn frames(&self) -> u64 {
        self.capturer.frames_captured()
    }

    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.capturer.stop()?;
        self.peer.lock().await.close().await
    }
}
