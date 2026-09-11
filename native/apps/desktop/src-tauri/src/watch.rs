//! Assistir sem WebRTC na janela: a mídia chega por RTP puro (`PlainReceiver`) e quem
//! decodifica e desenha é o GStreamer, numa janela própria ao lado do app.
//!
//! É o caminho do Linux, onde o WebKitGTK das distros vem sem WebRTC. Nos outros
//! sistemas o webview assiste sozinho e isto nunca é chamado.
//!
//! ponytail: janela do `autovideosink`, fora do app, sem título nem controles. Desenhar
//! dentro da janela do Tauri é o passo seguinte, se alguém pedir.

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, anyhow};
use media::PlainReceiver;

pub struct Watch {
    receiver: PlainReceiver,
    player: Child,
}

#[derive(Default)]
pub struct Watches {
    /// A chave SRTP deste lado, uma só para o app inteiro. Só protege o pacote que abre
    /// o caminho no roteador; cada receptor sorteia o próprio SSRC.
    key: Option<[u8; 30]>,
    active: HashMap<String, Watch>,
}

impl Watches {
    pub fn key(&mut self) -> [u8; 30] {
        *self.key.get_or_insert_with(|| std::array::from_fn(|_| rand::random()))
    }

    pub fn start(
        &mut self,
        peer_id: String,
        address: &str,
        server_key: &[u8],
        video_payload: Option<u8>,
        audio_payload: Option<u8>,
    ) -> Result<()> {
        if self.active.contains_key(&peer_id) {
            return Ok(());
        }

        let key = self.key();
        let video = match video_payload {
            Some(payload) => Some((payload, free_port()?)),
            None => None,
        };
        let audio = match audio_payload {
            Some(payload) => Some((payload, free_port()?)),
            None => None,
        };

        let mut pipeline: Vec<String> = vec!["-q".into()];

        if let Some((payload, to)) = video {
            pipeline.extend(
                format!(
                    "udpsrc address=127.0.0.1 port={} caps=application/x-rtp,media=video,encoding-name=H264,clock-rate=90000,payload={payload} \
                     ! rtpjitterbuffer latency=80 ! rtph264depay ! h264parse ! avdec_h264 ! videoconvert ! autovideosink sync=false",
                    to.port()
                )
                .split_whitespace()
                .map(String::from),
            );
        }

        if let Some((payload, to)) = audio {
            pipeline.extend(
                format!(
                    "udpsrc address=127.0.0.1 port={} caps=application/x-rtp,media=audio,encoding-name=OPUS,clock-rate=48000,payload={payload} \
                     ! rtpjitterbuffer latency=80 ! rtpopusdepay ! opusdec ! audioconvert ! audioresample ! autoaudiosink",
                    to.port()
                )
                .split_whitespace()
                .map(String::from),
            );
        }

        let mut player = Command::new("gst-launch-1.0")
            .args(&pipeline)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .context("gst-launch-1.0 não abriu; instale gstreamer1.0-tools e os plugins good/libav")?;

        let receiver = match PlainReceiver::start(address, &key, server_key, video, audio) {
            Ok(receiver) => receiver,
            Err(error) => {
                let _ = player.kill();

                return Err(anyhow!("{error}"));
            }
        };

        tracing::info!(peer = %peer_id, %address, "assistindo por RTP puro numa janela do GStreamer");

        self.active.insert(peer_id, Watch { receiver, player });

        Ok(())
    }

    pub fn stop(&mut self, peer_id: Option<&str>) {
        let keys: Vec<String> = match peer_id {
            Some(peer_id) => vec![peer_id.to_string()],
            None => self.active.keys().cloned().collect(),
        };

        for key in keys {
            if let Some(mut watch) = self.active.remove(&key) {
                watch.receiver.stop();
                let _ = watch.player.kill();
                let _ = watch.player.wait();
            }
        }
    }

    pub fn packets(&self, peer_id: &str) -> u64 {
        self.active.get(peer_id).map(|watch| watch.receiver.packets()).unwrap_or(0)
    }
}

/// Uma porta local livre para o `udpsrc`. Abrir e fechar tem uma janela de corrida
/// teórica; na prática ninguém mais pega uma porta efêmera neste milissegundo.
fn free_port() -> Result<SocketAddr> {
    Ok(UdpSocket::bind("127.0.0.1:0")?.local_addr()?)
}
