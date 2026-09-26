//! Assistir sem GStreamer: o RTP aberto vira quadro H.264 e PCM, e a interface decodifica
//! com o que o sistema tem (VideoToolbox no macOS, Media Foundation no Windows).
//!
//! É o mesmo desenho do `watching.rs` do Linux — um `PlainReceiver` para a sala inteira, uma
//! rota por producer —, trocando o `gst-launch` filho por uma thread que remonta o quadro.
//! O que sai daqui vai para uma fila só, e a interface a esvazia no ritmo dela.

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use media::{AudioUnpacker, Counters, PlainReceiver, Rtx, Stream, VideoUnpacker};
use serde_json::Value;

/// Quadros e blocos de som esperando a interface. Dois segundos de uma tela a 60 fps com o
/// som junto; passou disso a interface não está acompanhando, e guardar mais só atrasaria.
const QUEUE: usize = 256;

/// De quanto em quanto tempo a thread olha se mandaram parar.
const PATIENCE: Duration = Duration::from_millis(200);

/// O maior datagrama que o `PlainReceiver` repassa.
const DATAGRAM: usize = 1_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// `timestamp` no relógio de 90 kHz do RTP.
    Video { keyframe: bool, timestamp: u32 },
    /// PCM `f32` little-endian, estéreo intercalado, 48 kHz.
    Audio,
}

/// Um quadro H.264 em Annex-B ou um bloco de som, de um producer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    pub producer_id: String,
    pub kind: MediaKind,
    pub data: Vec<u8>,
}

/// O que o `consumePlain` respondeu sobre uma transmissão.
pub struct Incoming<'a> {
    pub producer_id: String,
    pub kind: &'a str,
    pub address: &'a str,
    pub server_key: &'a [u8],
    pub payload_type: u8,
    /// O que o servidor devolveu; sem ele o receptor aprende no primeiro pacote.
    pub ssrc: Option<u32>,
    /// Som de tela compartilhada, que chega mudo por regra.
    pub always_muted: bool,
    /// A retransmissão do servidor: é por ela que pacote perdido volta.
    pub rtx: Option<Rtx>,
}

/// O `rtx` da resposta do `consumePlain`, quando o servidor anuncia um.
pub fn rtx_of(answer: &Value) -> Option<Rtx> {
    Some(Rtx {
        ssrc: u32::try_from(answer["rtx"]["ssrc"].as_u64()?).ok()?,
        payload_type: u8::try_from(answer["rtx"]["payloadType"].as_u64()?).ok()?,
    })
}

struct Watch {
    stop: Arc<AtomicBool>,
    video: bool,
    always_muted: bool,
}

pub struct Watching {
    /// A chave SRTP deste lado, uma só para o app inteiro.
    key: Option<[u8; 30]>,
    /// O socket da sessão, aberto no primeiro producer e fechado com o último.
    receiver: Option<PlainReceiver>,
    active: HashMap<String, Watch>,
    deafened: bool,
    out: SyncSender<Media>,
}

impl Watching {
    /// Devolve também a ponta que a interface esvazia.
    pub fn new() -> (Self, Receiver<Media>) {
        let (out, queue) = sync_channel(QUEUE);

        (
            Self {
                key: None,
                receiver: None,
                active: HashMap::new(),
                deafened: false,
                out,
            },
            queue,
        )
    }

    /// A chave que vai ao servidor no `consumePlain`.
    pub fn key(&mut self) -> [u8; 30] {
        *self
            .key
            .get_or_insert_with(media::PlainSender::generate_key)
    }

    pub fn is_watching(&self, producer_id: &str) -> bool {
        self.active.contains_key(producer_id)
    }

    pub fn start(&mut self, incoming: Incoming<'_>) -> Result<()> {
        let Incoming {
            producer_id,
            kind,
            address,
            server_key,
            payload_type,
            ssrc,
            always_muted,
            rtx,
        } = incoming;

        // Outro endereço é outra sessão no servidor: o que estava aberto já morreu lá.
        if self
            .receiver
            .as_ref()
            .is_some_and(|receiver| media::resolve(address).ok() != Some(receiver.server()))
        {
            self.stop(None);
        }

        if self.active.contains_key(&producer_id) {
            return Ok(());
        }

        let key = self.key();

        if self.receiver.is_none() {
            self.receiver = Some(
                PlainReceiver::start(address, &key, server_key)
                    .map_err(|error| anyhow!("{error}"))?,
            );
        }

        let socket =
            UdpSocket::bind("127.0.0.1:0").context("sem porta local para a transmissão")?;
        let to = socket.local_addr()?;
        let video = kind == "video";
        let stop = Arc::new(AtomicBool::new(false));

        socket.set_read_timeout(Some(PATIENCE))?;
        pump(
            socket,
            producer_id.clone(),
            video,
            Arc::clone(&stop),
            self.out.clone(),
        )?;

        if let Some(receiver) = self.receiver.as_ref() {
            receiver.route(Stream {
                id: producer_id.clone(),
                payload_type,
                to,
                ssrc,
                video,
                rtx,
            });
        }

        if always_muted || (self.deafened && !video) {
            self.set_muted(&producer_id, true);
        }

        tracing::info!(producer = %producer_id, kind, "assistindo por RTP puro");
        self.active.insert(
            producer_id,
            Watch {
                stop,
                video,
                always_muted,
            },
        );

        Ok(())
    }

    /// `None` fecha a sessão inteira. O socket só sai nesse caso: o SFU manda para o
    /// endereço que o `comedia` aprendeu, e um socket novo não recebe mais nada naquela sala.
    pub fn stop(&mut self, producer_id: Option<&str>) {
        let keys: Vec<String> = match producer_id {
            Some(producer_id) => vec![producer_id.to_owned()],
            None => self.active.keys().cloned().collect(),
        };

        for key in keys {
            if let Some(watch) = self.active.remove(&key) {
                watch.stop.store(true, Ordering::Relaxed);
            }

            if let Some(receiver) = self.receiver.as_ref() {
                receiver.unroute(&key);
            }
        }

        if producer_id.is_none() {
            self.receiver = None;
        }
    }

    /// Mudo é não repassar o pacote: o decodificador só vê silêncio.
    pub fn set_muted(&self, producer_id: &str, muted: bool) {
        if let Some(receiver) = self.receiver.as_ref() {
            receiver.set_muted(producer_id, muted);
        }
    }

    /// Ensurdecer cala só o áudio: pausar o vídeo faria esperar keyframe na volta.
    pub fn deafen(&mut self, deafened: bool) {
        self.deafened = deafened;

        for (producer_id, watch) in &self.active {
            if !watch.video {
                self.set_muted(producer_id, deafened || watch.always_muted);
            }
        }
    }

    pub fn is_deafened(&self) -> bool {
        self.deafened
    }

    /// O que aconteceu com o vídeo de uma transmissão: recebidos, recuperados e perdidos.
    pub fn counters(&self, producer_id: &str) -> Option<Counters> {
        self.receiver.as_ref()?.counters(producer_id)
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.stop(None);
    }
}

/// A thread de um producer: lê o RTP que o receptor repassou e põe na fila o que remontou.
///
/// ponytail: fila cheia descarta o que chegou. Para o vídeo isso é imagem parada até o
/// próximo keyframe; a saída é a interface esvaziar mais rápido, não uma fila maior.
fn pump(
    socket: UdpSocket,
    producer_id: String,
    video: bool,
    stop: Arc<AtomicBool>,
    out: SyncSender<Media>,
) -> Result<()> {
    let mut audio = if video {
        None
    } else {
        Some(AudioUnpacker::new()?)
    };
    let mut frames = VideoUnpacker::default();

    std::thread::Builder::new()
        .name(format!("watch-{producer_id}"))
        .spawn(move || {
            let mut datagram = [0_u8; DATAGRAM];

            while !stop.load(Ordering::Relaxed) {
                let Ok(size) = socket.recv(&mut datagram) else {
                    continue;
                };

                let packet = &datagram[..size];

                let media = match audio.as_mut() {
                    Some(audio) => audio.push(packet).map(|samples| Media {
                        producer_id: producer_id.clone(),
                        kind: MediaKind::Audio,
                        data: samples
                            .iter()
                            .flat_map(|sample| sample.to_le_bytes())
                            .collect(),
                    }),
                    None => frames.push(packet).map(|unit| Media {
                        producer_id: producer_id.clone(),
                        kind: MediaKind::Video {
                            keyframe: unit.keyframe,
                            timestamp: unit.timestamp,
                        },
                        data: unit.data,
                    }),
                };

                if let Some(TrySendError::Disconnected(_)) =
                    media.and_then(|media| out.try_send(media).err())
                {
                    return;
                }
            }
        })?;

    Ok(())
}
