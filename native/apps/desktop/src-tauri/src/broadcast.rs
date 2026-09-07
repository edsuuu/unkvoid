//! Connects capture, encoding, and transport.
//!
//! One encoder feeds N connections: the frame is compressed **once** and sent to
//! every viewer. Encoding per viewer would overwhelm the broadcaster's machine —
//! P2P costs upload bandwidth, not CPU.

use std::collections::HashMap;
use std::sync::Arc;

use capture::{CaptureConfig, CaptureEvent, PlatformCapturer, Quality};
use media::{AudioEncoder, EncodedFrame, EncoderConfig, PeerLink, PlatformEncoder, Signal};
use tokio::sync::{Mutex, mpsc};

/// Above this limit, the broadcaster's upload multiplies: 4 viewers at 1080p
/// already need ~28 Mbps upstream. Beyond this, the SFU is more efficient.
pub const LIMITE_P2P: usize = 3;

type Peers = Arc<Mutex<HashMap<String, PeerLink>>>;

pub struct Broadcast {
    capturer: PlatformCapturer,
    peers: Peers,
    ice_servers: Vec<String>,
    frame_rate: f64,
    signals: mpsc::Sender<(String, Signal)>,
}

impl Broadcast {
    /// Starts capture and the encoder. Connections are created afterward, one per viewer.
    pub fn start(
        quality: Quality,
        ice_servers: Vec<String>,
    ) -> anyhow::Result<(Self, mpsc::Receiver<(String, Signal)>)> {
        let encoder_config = EncoderConfig::for_quality(quality);
        let peers: Peers = Arc::new(Mutex::new(HashMap::new()));

        // The capture callback is Fn: the encoder keeps state between frames and
        // needs interior mutability.
        let encoder = std::sync::Mutex::new(PlatformEncoder::new(&encoder_config)?);
        let audio = std::sync::Mutex::new(AudioEncoder::new(96_000)?);
        let destino = Arc::clone(&peers);
        let runtime = tokio::runtime::Handle::current();

        let capturer = PlatformCapturer::start(
            &CaptureConfig {
                quality,
                ..CaptureConfig::default()
            },
            move |evento| {
                let quadro = match evento {
                    CaptureEvent::Video(quadro) => quadro,
                    CaptureEvent::Audio(bloco) => {
                        let Ok(mut audio) = audio.lock() else {
                            return;
                        };

                        let Ok(pacotes) = audio.push(&bloco) else {
                            return;
                        };

                        drop(audio);

                        if pacotes.is_empty() {
                            return;
                        }

                        let peers = Arc::clone(&destino);

                        runtime.spawn(async move { difundir_audio(&peers, pacotes).await });

                        return;
                    }
                };

                let Some(surface) = quadro.surface.as_ref() else {
                    return;
                };

                let codificado = {
                    let Ok(mut encoder) = encoder.lock() else {
                        return;
                    };

                    match encoder.encode(surface, quadro.timestamp_ns) {
                        Ok(codificado) => codificado,
                        Err(_) => return,
                    }
                };

                let peers = Arc::clone(&destino);

                runtime.spawn(async move { difundir(&peers, codificado).await });
            },
        )?;

        let (emissor, receptor) = mpsc::channel(128);

        Ok((
            Self {
                capturer,
                peers,
                ice_servers,
                frame_rate: encoder_config.frame_rate,
                signals: emissor,
            },
            receptor,
        ))
    }

    /// Creates a viewer connection and returns the offer they need to receive.
    pub async fn offer_to(&self, peer_id: String) -> anyhow::Result<String> {
        let mut peers = self.peers.lock().await;

        if peers.len() >= LIMITE_P2P {
            anyhow::bail!("P2P supports only {LIMITE_P2P} viewers — use the SFU above that limit");
        }

        let (peer, mut sinais) =
            PeerLink::connect(self.ice_servers.clone(), self.frame_rate).await?;
        let oferta = peer.create_offer().await?;

        peers.insert(peer_id.clone(), peer);

        // Each connection has its own candidates, and each goes only to its owner.
        let saida = self.signals.clone();

        tokio::spawn(async move {
            while let Some(sinal) = sinais.recv().await {
                if saida.send((peer_id.clone(), sinal)).await.is_err() {
                    break;
                }
            }
        });

        Ok(oferta)
    }

    pub async fn accept_answer(&self, peer_id: &str, sdp: String) -> anyhow::Result<()> {
        let mut peers = self.peers.lock().await;

        peers
            .get_mut(peer_id)
            .ok_or_else(|| anyhow::anyhow!("no connection for {peer_id}"))?
            .accept_answer(sdp)
            .await
    }

    pub async fn add_candidate(&self, peer_id: &str, json: String) -> anyhow::Result<()> {
        let peers = self.peers.lock().await;

        peers
            .get(peer_id)
            .ok_or_else(|| anyhow::anyhow!("no connection for {peer_id}"))?
            .add_candidate(json)
            .await
    }

    pub async fn drop_peer(&self, peer_id: &str) {
        if let Some(peer) = self.peers.lock().await.remove(peer_id) {
            let _ = peer.close().await;
        }
    }

    pub async fn viewers(&self) -> usize {
        self.peers.lock().await.len()
    }

    pub fn frames(&self) -> u64 {
        self.capturer.frames_captured()
    }

    pub async fn stop(&mut self) -> anyhow::Result<()> {
        self.capturer.stop()?;

        for (_, peer) in self.peers.lock().await.drain() {
            let _ = peer.close().await;
        }

        Ok(())
    }
}

/// The same frame goes to everyone. A failure for one viewer does not affect the others.
async fn difundir(peers: &Peers, quadro: EncodedFrame) {
    let peers = peers.lock().await;

    for peer in peers.values() {
        let _ = peer.send_frame(&quadro).await;
    }
}

/// Audio follows the same path: compressed once and sent to everyone.
async fn difundir_audio(peers: &Peers, pacotes: Vec<Vec<u8>>) {
    let peers = peers.lock().await;

    for peer in peers.values() {
        for pacote in &pacotes {
            let _ = peer.send_audio(pacote).await;
        }
    }
}
