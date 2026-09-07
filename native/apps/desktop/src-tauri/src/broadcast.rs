//! Connects capture, encoding, and transport.
//!
//! One encoder feeds N connections: the frame is compressed **once** and sent to
//! every viewer. Encoding per viewer would overwhelm the broadcaster's machine —
//! P2P costs upload bandwidth, not CPU.

use std::collections::HashMap;
use std::sync::Arc;

use capture::{CaptureConfig, CaptureEvent, PlatformCapturer, Quality};
use media::{
    AudioEncoder, EncodedFrame, EncoderConfig, PeerLink, PlainSender, PlatformEncoder, Signal,
};
use tokio::sync::{Mutex, mpsc};

/// Above this limit, the broadcaster's upload multiplies: 4 viewers at 1080p
/// already need ~28 Mbps upstream. Beyond this, the SFU is more efficient.
pub const LIMITE_P2P: usize = 3;

type Peers = Arc<Mutex<HashMap<String, PeerLink>>>;
type Sfu = Arc<Mutex<Option<PlainSender>>>;

pub struct Broadcast {
    capturer: PlatformCapturer,
    peers: Peers,
    sfu: Sfu,
    sfu_key: [u8; 30],
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
        let sfu: Sfu = Arc::new(Mutex::new(None));
        let destino = Arc::clone(&peers);
        let destino_sfu = Arc::clone(&sfu);
        let runtime = tokio::runtime::Handle::current();
        let frame_rate = encoder_config.frame_rate;

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
                        let sfu = Arc::clone(&destino_sfu);

                        runtime.spawn(async move { difundir_audio(&peers, &sfu, pacotes).await });

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
                let sfu = Arc::clone(&destino_sfu);

                runtime.spawn(async move { difundir(&peers, &sfu, codificado, frame_rate).await });
            },
        )?;

        let (emissor, receptor) = mpsc::channel(128);

        Ok((
            Self {
                capturer,
                peers,
                sfu,
                sfu_key: PlainSender::generate_key(),
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

    /// What the server needs before the first packet, including the key that protects
    /// it. The key is generated when the broadcast starts and never changes: it is the
    /// same context that numbers the packets.
    pub fn sfu_offer(&self, kind: &str) -> serde_json::Value {
        serde_json::json!({
            "rtpParameters": PlainSender::rtp_parameters(kind),
            "srtpParameters": {
                "cryptoSuite": PlainSender::CRYPTO_SUITE,
                "keyBase64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, self.sfu_key),
            },
        })
    }

    /// Switches the broadcast to the server. From here the upload is constant no matter
    /// how many people watch — which is the whole reason to give up the direct path.
    pub async fn use_sfu(&self, address: String) -> anyhow::Result<()> {
        let sender = PlainSender::connect(address.as_str(), &self.sfu_key)?;

        *self.sfu.lock().await = Some(sender);

        for (_, peer) in self.peers.lock().await.drain() {
            let _ = peer.close().await;
        }

        Ok(())
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

        *self.sfu.lock().await = None;

        Ok(())
    }
}

/// The same frame goes to everyone. A failure for one viewer does not affect the others.
///
/// Only one of the two paths is ever populated: turning on the SFU closes the direct
/// connections, because uploading to both is exactly the cost the SFU exists to avoid.
async fn difundir(peers: &Peers, sfu: &Sfu, quadro: EncodedFrame, frame_rate: f64) {
    for peer in peers.lock().await.values() {
        let _ = peer.send_frame(&quadro).await;
    }

    if let Some(sender) = sfu.lock().await.as_mut() {
        let _ = sender.send_frame(&quadro, frame_rate);
    }
}

/// Audio follows the same path: compressed once and sent to everyone.
async fn difundir_audio(peers: &Peers, sfu: &Sfu, pacotes: Vec<Vec<u8>>) {
    for peer in peers.lock().await.values() {
        for pacote in &pacotes {
            let _ = peer.send_audio(pacote).await;
        }
    }

    if let Some(sender) = sfu.lock().await.as_mut() {
        for pacote in &pacotes {
            let _ = sender.send_audio(pacote);
        }
    }
}
