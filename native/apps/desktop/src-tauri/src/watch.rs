//! Assistir sem WebRTC na janela: a mídia chega por RTP puro (`PlainReceiver`), o
//! GStreamer decodifica, e o quadro entra no app como MJPEG numa `<img>` — um
//! servidorzinho HTTP em 127.0.0.1 que o webview lê. O som vai direto para a saída.
//!
//! É o caminho do Linux, onde o WebKitGTK das distros vem sem WebRTC. Nos outros
//! sistemas o webview assiste sozinho e isto nunca é chamado.
//!
//! ponytail: MJPEG custa CPU (decodifica H.264 e recodifica JPEG) e não tem som na
//! `<img>`. O passo seguinte é fMP4 por MSE, sem recodificar, se a CPU pesar.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use media::PlainReceiver;

pub struct Watch {
    receiver: PlainReceiver,
    player: Child,
    /// Porta do MJPEG em 127.0.0.1, para a `<img>` do cartão.
    port: u16,
    stop: Arc<AtomicBool>,
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
    ) -> Result<u16> {
        if let Some(watch) = self.active.get(&peer_id) {
            return Ok(watch.port);
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
                     ! rtpjitterbuffer latency=80 ! rtph264depay ! h264parse ! avdec_h264 ! videoconvert \
                     ! jpegenc quality=85 ! multipartmux boundary=unkvoid ! fdsink fd=1",
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
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("gst-launch-1.0 não abriu; instale gstreamer1.0-tools e os plugins good/libav")?;

        let stop = Arc::new(AtomicBool::new(false));
        let port = match player.stdout.take() {
            Some(stdout) => serve_mjpeg(stdout, Arc::clone(&stop)),
            None => Err(anyhow!("o gst-launch abriu sem stdout")),
        };

        let port = match port {
            Ok(port) => port,
            Err(error) => {
                let _ = player.kill();

                return Err(error);
            }
        };

        let receiver = match PlainReceiver::start(address, &key, server_key, video, audio) {
            Ok(receiver) => receiver,
            Err(error) => {
                let _ = player.kill();

                return Err(anyhow!("{error}"));
            }
        };

        tracing::info!(peer = %peer_id, %address, port, "assistindo por RTP puro, MJPEG em 127.0.0.1");

        self.active.insert(peer_id, Watch { receiver, player, port, stop });

        Ok(port)
    }

    pub fn stop(&mut self, peer_id: Option<&str>) {
        let keys: Vec<String> = match peer_id {
            Some(peer_id) => vec![peer_id.to_string()],
            None => self.active.keys().cloned().collect(),
        };

        for key in keys {
            if let Some(mut watch) = self.active.remove(&key) {
                watch.stop.store(true, Ordering::Relaxed);
                watch.receiver.stop();
                let _ = watch.player.kill();
                let _ = watch.player.wait();
            }
        }
    }

    pub fn set_muted(&self, peer_id: &str, muted: bool) {
        if let Some(watch) = self.active.get(peer_id) {
            watch.receiver.set_muted(muted);
        }
    }

    pub fn packets(&self, peer_id: &str) -> u64 {
        self.active.get(peer_id).map(|watch| watch.receiver.packets()).unwrap_or(0)
    }
}

/// O multipart que o `multipartmux` escreve vira uma resposta HTTP que nunca acaba.
/// Um cliente por vez: é a `<img>` do cartão. Quem conectar depois toma o lugar.
fn serve_mjpeg(mut stdout: impl Read + Send + 'static, stop: Arc<AtomicBool>) -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("could not open the MJPEG port")?;
    let port = listener.local_addr()?.port();
    let client: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));

    listener.set_nonblocking(true)?;

    let client_accept = Arc::clone(&client);
    let stop_accept = Arc::clone(&stop);

    std::thread::spawn(move || {
        while ! stop_accept.load(Ordering::Relaxed) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            };

            let mut request = [0_u8; 1024];
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = stream.read(&mut request);

            let headers = "HTTP/1.1 200 OK\r\n\
                Content-Type: multipart/x-mixed-replace; boundary=unkvoid\r\n\
                Cache-Control: no-cache\r\nConnection: close\r\n\r\n";

            if stream.write_all(headers.as_bytes()).is_ok()
                && let Ok(mut current) = client_accept.lock()
            {
                *current = Some(stream);
            }
        }
    });

    std::thread::spawn(move || {
        let mut chunk = vec![0_u8; 64 * 1024];

        loop {
            let read = match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };

            if stop.load(Ordering::Relaxed) {
                break;
            }

            if let Ok(mut current) = client.lock()
                && let Some(stream) = current.as_mut()
                && stream.write_all(&chunk[..read]).is_err()
            {
                *current = None;
            }
        }
    });

    Ok(port)
}

/// Uma porta local livre para o `udpsrc`. Abrir e fechar tem uma janela de corrida
/// teórica; na prática ninguém mais pega uma porta efêmera neste milissegundo.
fn free_port() -> Result<SocketAddr> {
    Ok(UdpSocket::bind("127.0.0.1:0")?.local_addr()?)
}
