//! Liga captura, encoder e transporte.
//!
//! Um encoder alimenta N conexões: o quadro é comprimido **uma vez** e enviado a
//! cada espectador. Codificar por espectador derreteria a máquina de quem
//! transmite — o custo do P2P é banda de upload, não CPU.

use std::collections::HashMap;
use std::sync::Arc;

use capture::{CaptureConfig, CaptureEvent, PlatformCapturer, Quality};
use media::{AudioEncoder, EncodedFrame, EncoderConfig, PeerLink, PlatformEncoder, Signal};
use tokio::sync::{Mutex, mpsc};

/// Acima disso o upload de quem transmite multiplica: 4 espectadores em 1080p já
/// pedem ~28 Mbps de subida. Passou daqui, o SFU compensa.
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
    /// Sobe a captura e o encoder. As conexões nascem depois, uma por espectador.
    pub fn start(
        quality: Quality,
        ice_servers: Vec<String>,
    ) -> anyhow::Result<(Self, mpsc::Receiver<(String, Signal)>)> {
        let encoder_config = EncoderConfig::for_quality(quality);
        let peers: Peers = Arc::new(Mutex::new(HashMap::new()));

        // O callback da captura é Fn: o encoder guarda estado entre quadros e
        // precisa de mutabilidade interior.
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

    /// Cria a conexão para um espectador e devolve a oferta que ele precisa receber.
    pub async fn offer_to(&self, peer_id: String) -> anyhow::Result<String> {
        let mut peers = self.peers.lock().await;

        if peers.len() >= LIMITE_P2P {
            anyhow::bail!("P2P só até {LIMITE_P2P} espectadores — acima disso use o SFU");
        }

        let (peer, mut sinais) =
            PeerLink::connect(self.ice_servers.clone(), self.frame_rate).await?;
        let oferta = peer.create_offer().await?;

        peers.insert(peer_id.clone(), peer);

        // Cada conexão tem seus candidatos, e cada um vai só para o dono dela.
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
            .ok_or_else(|| anyhow::anyhow!("não há conexão com {peer_id}"))?
            .accept_answer(sdp)
            .await
    }

    pub async fn add_candidate(&self, peer_id: &str, json: String) -> anyhow::Result<()> {
        let peers = self.peers.lock().await;

        peers
            .get(peer_id)
            .ok_or_else(|| anyhow::anyhow!("não há conexão com {peer_id}"))?
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

/// Mesmo quadro para todos. Falha em um espectador não derruba os outros.
async fn difundir(peers: &Peers, quadro: EncodedFrame) {
    let peers = peers.lock().await;

    for peer in peers.values() {
        let _ = peer.send_frame(&quadro).await;
    }
}

/// O áudio segue o mesmo caminho: comprimido uma vez, enviado a todos.
async fn difundir_audio(peers: &Peers, pacotes: Vec<Vec<u8>>) {
    let peers = peers.lock().await;

    for peer in peers.values() {
        for pacote in &pacotes {
            let _ = peer.send_audio(pacote).await;
        }
    }
}
