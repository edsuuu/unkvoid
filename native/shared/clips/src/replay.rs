//! O buffer do replay: tudo o que foi codificado nos últimos minutos, em disco.
//!
//! Em disco e não na RAM, como o ShadowPlay: trinta minutos de 1080p60 a 35 Mb/s são quase
//! 8 GB, e segurar isso na memória seria o app pesando mais que o jogo. O disco recebe uns
//! 4 MB/s, o que um NVMe nem sente.
//!
//! São arquivos de uns dez segundos, cada um começando num quadro-chave: jogar fora o que
//! passou do tempo é apagar o arquivo mais velho, sem reescrever nada. Cada arquivo é uma
//! sequência de registros `[flags u8][tempo u64][tamanho u32][dados]`, na ordem em que o
//! vídeo e o som chegaram.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Context;

/// Duração mínima de cada arquivo. Ele só fecha num quadro-chave, que vem a cada 2 s.
const SEGMENT_NS: u64 = 10_000_000_000;

const SEGMENT_EXTENSION: &str = "segment";

const AUDIO_FLAG: u8 = 1;
const KEYFRAME_FLAG: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Video,
    Audio,
}

pub struct Record {
    pub track: Track,
    pub keyframe: bool,
    pub timestamp_ns: u64,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub path: PathBuf,
    pub start_ns: u64,

    /// Até onde o arquivo estava escrito quando a foto foi tirada. O último continua
    /// crescendo enquanto o clipe é montado; ler além disto pegaria um registro pela metade.
    pub length: u64,
}

enum Message {
    Record(Record),
    Snapshot(Sender<Vec<Segment>>),
    SetWindow(Duration),
    Clear,
    Stop,
}

/// Quem produz vídeo ou som empurra registros por aqui. Nunca bloqueia: o canal não tem
/// limite, e é a thread do buffer que paga o disco.
#[derive(Clone)]
pub struct RecordSink(Sender<Message>);

impl RecordSink {
    pub fn push(&self, record: Record) {
        let _ = self.0.send(Message::Record(record));
    }
}

pub struct ReplayBuffer {
    messages: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

impl ReplayBuffer {
    /// Começa vazio: o que sobrou de uma sessão anterior (queda de energia, app morto) é
    /// apagado, porque o relógio daquela sessão não tem nada a ver com o desta.
    pub fn start(folder: PathBuf, window: Duration) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&folder).with_context(|| format!("criando {}", folder.display()))?;

        for entry in std::fs::read_dir(&folder)?.flatten() {
            if entry.path().extension().is_some_and(|extension| extension == SEGMENT_EXTENSION) {
                let _ = std::fs::remove_file(entry.path());
            }
        }

        let (messages, inbox) = channel();
        let worker = std::thread::Builder::new()
            .name("replay-buffer".into())
            .spawn(move || Worker::new(folder, window).run(inbox))?;

        Ok(Self { messages, worker: Some(worker) })
    }

    pub fn sink(&self) -> RecordSink {
        RecordSink(self.messages.clone())
    }

    /// Os arquivos que cobrem o buffer agora, do mais velho ao mais novo, com o último já
    /// descarregado no disco.
    pub fn snapshot(&self) -> anyhow::Result<Vec<Segment>> {
        let (reply, answer) = channel();

        self.messages.send(Message::Snapshot(reply)).context("o buffer do replay parou")?;

        answer.recv().context("o buffer do replay parou")
    }

    pub fn set_window(&self, window: Duration) {
        let _ = self.messages.send(Message::SetWindow(window));
    }

    /// Joga fora tudo o que está no buffer.
    pub fn clear(&self) {
        let _ = self.messages.send(Message::Clear);
    }
}

impl Drop for ReplayBuffer {
    fn drop(&mut self) {
        let _ = self.messages.send(Message::Stop);

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Worker {
    folder: PathBuf,
    window_ns: u64,
    segments: VecDeque<Segment>,
    writer: Option<BufWriter<File>>,
    next_number: u64,
}

impl Worker {
    fn new(folder: PathBuf, window: Duration) -> Self {
        Self {
            folder,
            window_ns: window.as_nanos() as u64,
            segments: VecDeque::new(),
            writer: None,
            next_number: 0,
        }
    }

    fn run(mut self, inbox: Receiver<Message>) {
        while let Ok(message) = inbox.recv() {
            let result = match message {
                Message::Record(record) => self.write(record),
                Message::Snapshot(reply) => self.flush().map(|()| {
                    let _ = reply.send(self.segments.iter().cloned().collect());
                }),
                Message::SetWindow(window) => {
                    self.window_ns = window.as_nanos() as u64;

                    Ok(())
                }
                Message::Clear => {
                    self.clear();

                    Ok(())
                }
                Message::Stop => break,
            };

            // Disco cheio ou arquivo travado por antivírus: perder um registro é melhor que
            // derrubar a gravação inteira. O próximo quadro-chave abre arquivo novo.
            if let Err(error) = result {
                tracing::error!(error = %error, "replay: falha escrevendo no buffer");

                self.writer = None;
            }
        }

        let _ = self.flush();
    }

    fn write(&mut self, record: Record) -> anyhow::Result<()> {
        let starts_segment = record.track == Track::Video
            && record.keyframe
            && self
                .segments
                .back()
                .is_none_or(|current| self.writer.is_none() || record.timestamp_ns >= current.start_ns + SEGMENT_NS);

        if starts_segment {
            self.rotate(record.timestamp_ns)?;
        }

        // Antes do primeiro quadro-chave não há o que decodificar: o registro não serve.
        let (Some(writer), Some(current)) = (self.writer.as_mut(), self.segments.back_mut()) else {
            return Ok(());
        };

        let flags = match record.track {
            Track::Video => 0,
            Track::Audio => AUDIO_FLAG,
        } | if record.keyframe { KEYFRAME_FLAG } else { 0 };

        writer.write_all(&[flags])?;
        writer.write_all(&record.timestamp_ns.to_le_bytes())?;
        writer.write_all(&(record.data.len() as u32).to_le_bytes())?;
        writer.write_all(&record.data)?;
        current.length += 13 + record.data.len() as u64;

        Ok(())
    }

    fn rotate(&mut self, start_ns: u64) -> anyhow::Result<()> {
        self.flush()?;

        let path = self.folder.join(format!("{:010}.{SEGMENT_EXTENSION}", self.next_number));

        self.next_number += 1;
        self.writer = Some(BufWriter::with_capacity(1 << 20, File::create(&path)?));
        self.segments.push_back(Segment { path, start_ns, length: 0 });

        // O mais velho só sai quando o seguinte já cobre o começo da janela sozinho: o
        // clipe começa num quadro-chave, e o de antes do corte mora no arquivo mais velho.
        let cutoff = start_ns.saturating_sub(self.window_ns);

        while self.segments.len() > 2 && self.segments[1].start_ns <= cutoff {
            if let Some(expired) = self.segments.pop_front() {
                // O Rust abre arquivo no Windows com FILE_SHARE_DELETE: apagar um que está
                // sendo lido por um clipe funciona, e o leitor termina em paz.
                if let Err(error) = std::fs::remove_file(&expired.path) {
                    tracing::warn!(error = %error, path = %expired.path.display(), "replay: não apagou o arquivo vencido");
                }
            }
        }

        Ok(())
    }

    fn clear(&mut self) {
        self.writer = None;

        for segment in self.segments.drain(..) {
            let _ = std::fs::remove_file(&segment.path);
        }
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        if let Some(writer) = self.writer.as_mut() {
            writer.flush()?;
        }

        Ok(())
    }
}

/// Lê os registros de um arquivo do buffer, só até onde ele estava escrito na foto.
pub fn read_records(segment: &Segment) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Record>>> {
    let mut reader = BufReader::with_capacity(1 << 20, File::open(&segment.path)?.take(segment.length));

    Ok(std::iter::from_fn(move || {
        let mut header = [0_u8; 13];

        match reader.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return None,
            Err(error) => return Some(Err(error.into())),
        }

        let flags = header[0];
        let timestamp_ns = u64::from_le_bytes(header[1..9].try_into().expect("8 bytes"));
        let length = u32::from_le_bytes(header[9..13].try_into().expect("4 bytes"));
        let mut data = vec![0_u8; length as usize];

        if let Err(error) = reader.read_exact(&mut data) {
            return Some(Err(error.into()));
        }

        Some(Ok(Record {
            track: if flags & AUDIO_FLAG == 0 { Track::Video } else { Track::Audio },
            keyframe: flags & KEYFRAME_FLAG != 0,
            timestamp_ns,
            data,
        }))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(timestamp_s: u64, keyframe: bool) -> Record {
        Record { track: Track::Video, keyframe, timestamp_ns: timestamp_s * 1_000_000_000, data: vec![7; 3] }
    }

    #[test]
    fn keeps_only_what_covers_the_window_and_reads_it_back() {
        let folder = std::env::temp_dir().join(format!("unkvoid-clips-test-{}", std::process::id()));
        let buffer = ReplayBuffer::start(folder.clone(), Duration::from_secs(30)).unwrap();
        let sink = buffer.sink();

        // Nada antes do primeiro quadro-chave, depois um a cada 2 s por 100 s.
        sink.push(video(0, false));

        for second in 1..=100 {
            sink.push(video(second, second % 2 == 0));
        }

        let segments = buffer.snapshot().unwrap();
        let records: Vec<Record> = segments
            .iter()
            .flat_map(|segment| read_records(segment).unwrap())
            .collect::<anyhow::Result<_>>()
            .unwrap();

        // O último arquivo começou em 92 s; o corte é 62 s, coberto pelo arquivo de 62 s.
        assert_eq!(segments.first().unwrap().start_ns, 62_000_000_000);
        assert_eq!(records.first().unwrap().timestamp_ns, 62_000_000_000);
        assert!(records.first().unwrap().keyframe);
        assert_eq!(records.last().unwrap().timestamp_ns, 100_000_000_000);
        assert_eq!(records.len(), 39);

        drop(buffer);
        std::fs::remove_dir_all(folder).unwrap();
    }
}
