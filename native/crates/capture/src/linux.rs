//! Captura no Linux: X11 pelo GStreamer, já codificada.
//!
//! Não há encoder de hardware que funcione em toda distro, e ligar a biblioteca do
//! GStreamer ao binário exigiria as `-dev` no build e as `.so` certas em cada máquina.
//! Então o app fala com o `gst-launch-1.0` como processo: `ximagesrc` lê a tela, o
//! `x264enc` comprime, e o H.264 (Annex-B) chega por um pipe. O `.deb` já exige os
//! plugins; o `gstreamer1.0-tools` é a única dependência a mais.
//!
//! O que sai daqui NÃO é buffer de GPU: é o quadro pronto, e `PlatformEncoder` no
//! Linux só o repassa. É o jeito de encaixar no fluxo dos outros sistemas sem mexer
//! no `broadcast.rs`.
//!
//! ponytail: só X11 (`ximagesrc`). Numa sessão Wayland pura o `DISPLAY` não existe e a
//! lista de telas sai vazia; o caminho é `pipewiresrc` via portal quando alguém pedir.
//! Sem lista de janelas ainda pelo mesmo motivo.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::{
    AudioChunk, CaptureConfig, CaptureError, CaptureEvent, CaptureSource, Display, Quality,
    VideoFrame, Window,
};

/// Um quadro já em H.264 Annex-B, com SPS/PPS na frente de cada keyframe.
#[derive(Clone)]
pub struct EncodedVideo {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

/// 48 kHz estéreo em `f32`, 20 ms por bloco — o que o `AudioEncoder` espera.
const AUDIO_BLOCK_BYTES: usize = 48_000 / 50 * 2 * 4;

pub struct LinuxCapturer {
    video: Child,
    audio: Option<Child>,
    frames: Arc<AtomicU64>,
    audio_chunks: Arc<AtomicU64>,
}

impl LinuxCapturer {
    pub fn preview(source: CaptureSource) -> Result<Vec<u8>, CaptureError> {
        let _ = source;

        if std::env::var_os("DISPLAY").is_none() {
            return Ok(Vec::new());
        }

        let output = Command::new("gst-launch-1.0")
            .args(["-q", "ximagesrc", "use-damage=false", "num-buffers=1", "!", "videoconvert", "!", "videoscale", "!", "video/x-raw,width=320,pixel-aspect-ratio=1/1", "!", "jpegenc", "!", "fdsink", "fd=1"])
            .stderr(Stdio::null())
            .output();

        Ok(output.map(|output| output.stdout).unwrap_or_default())
    }

    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        if std::env::var_os("DISPLAY").is_none() {
            return Ok(Vec::new());
        }

        let (width, height) = screen_size().unwrap_or((0, 0));

        Ok(vec![Display { id: 1, width, height }])
    }

    pub fn windows() -> Result<Vec<Window>, CaptureError> {
        Ok(Vec::new())
    }

    pub fn start<F>(config: &CaptureConfig, on_event: F) -> Result<Self, CaptureError>
    where
        F: Fn(CaptureEvent) + Send + Sync + 'static,
    {
        if std::env::var_os("DISPLAY").is_none() {
            return Err(CaptureError::NoDisplay);
        }

        let (width, height) = config.quality.dimensions();
        let frame_rate = config.frame_rate.clamp(1, 60);
        let on_event = Arc::new(on_event);
        let frames = Arc::new(AtomicU64::new(0));
        let audio_chunks = Arc::new(AtomicU64::new(0));

        // Os mesmos tetos do `EncoderConfig`, em kbit/s, porque aqui o encoder é o x264.
        let bitrate = match config.quality {
            Quality::Hd720 => 5_000,
            Quality::Hd1080 => 10_000,
            Quality::Qhd1440 => 16_000,
        } * frame_rate
            / 60;

        // ponytail: sem pedido de keyframe por fora; um a cada segundo é o que quem entra
        // na sala espera no pior caso. `aud=true` é o que separa os quadros no pipe.
        let pipeline = format!(
            "ximagesrc use-damage=false show-pointer={} ! video/x-raw,framerate={frame_rate}/1 \
             ! videoconvert ! videoscale ! video/x-raw,width={width},pixel-aspect-ratio=1/1 \
             ! x264enc tune=zerolatency speed-preset=ultrafast byte-stream=true aud=true \
             key-int-max={frame_rate} bitrate={bitrate} threads=0 \
             ! video/x-h264,stream-format=byte-stream,profile=constrained-baseline \
             ! fdsink fd=1",
            config.show_cursor
        );

        let mut video = launch(&pipeline)?;
        let mut stdout = video.stdout.take().expect("stdout piped");
        let frames_thread = Arc::clone(&frames);
        let on_video = Arc::clone(&on_event);

        std::thread::spawn(move || {
            let started = Instant::now();
            let mut pending = Vec::new();
            let mut chunk = vec![0_u8; 64 * 1024];

            loop {
                let read = match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };

                pending.extend_from_slice(&chunk[..read]);

                while let Some(data) = take_access_unit(&mut pending) {
                    frames_thread.fetch_add(1, Ordering::Relaxed);

                    on_video(CaptureEvent::Video(VideoFrame {
                        width,
                        height,
                        timestamp_ns: started.elapsed().as_nanos() as u64,
                        surface: Some(EncodedVideo {
                            keyframe: has_idr(&data),
                            data,
                        }),
                    }));
                }
            }

            tracing::warn!("captura: o gst-launch de vídeo terminou");
        });

        let audio = if config.capture_audio {
            // O monitor da saída padrão é o som do sistema inteiro. Filtrar por app
            // (`MUTED_APPS`) não existe aqui.
            match launch(
                "pulsesrc device=@DEFAULT_MONITOR@ ! audioconvert ! audioresample \
                 ! audio/x-raw,format=F32LE,rate=48000,channels=2,layout=interleaved ! fdsink fd=1",
            ) {
                Ok(mut child) => {
                    let mut stdout = child.stdout.take().expect("stdout piped");
                    let chunks = Arc::clone(&audio_chunks);
                    let on_audio = Arc::clone(&on_event);

                    std::thread::spawn(move || {
                        let mut block = vec![0_u8; AUDIO_BLOCK_BYTES];

                        while stdout.read_exact(&mut block).is_ok() {
                            chunks.fetch_add(1, Ordering::Relaxed);

                            on_audio(CaptureEvent::Audio(AudioChunk {
                                sample_rate: 48_000,
                                channels: 2,
                                samples: block
                                    .chunks_exact(4)
                                    .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
                                    .collect(),
                            }));
                        }

                        tracing::warn!("captura: o gst-launch de áudio terminou (sem PulseAudio/PipeWire?)");
                    });

                    Some(child)
                }
                Err(error) => {
                    tracing::warn!(error = %error, "captura: áudio do sistema indisponível, vai sem som");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self { video, audio, frames, audio_chunks })
    }

    pub fn frames_captured(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn audio_chunks_captured(&self) -> u64 {
        self.audio_chunks.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        let _ = self.video.kill();
        let _ = self.video.wait();

        if let Some(audio) = self.audio.as_mut() {
            let _ = audio.kill();
            let _ = audio.wait();
        }

        Ok(())
    }
}

impl Drop for LinuxCapturer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn launch(pipeline: &str) -> Result<Child, CaptureError> {
    Command::new("gst-launch-1.0")
        .arg("-q")
        .args(pipeline.split_whitespace())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| {
            CaptureError::Platform(format!(
                "gst-launch-1.0 não abriu ({error}); instale gstreamer1.0-tools e os plugins good/ugly"
            ))
        })
}

/// Tamanho da tela pelo X, para o seletor mostrar. Sem `xdpyinfo` nem `xrandr` fica
/// sem número — a captura em si não depende disto.
fn screen_size() -> Option<(u32, u32)> {
    let text = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .stderr(Stdio::null())
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    };

    let pair = |numbers: &str| {
        let (width, height) = numbers.trim().split_once('x')?;

        Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
    };

    if let Some(output) = text("xdpyinfo", &[])
        && let Some(line) = output.lines().find(|line| line.trim_start().starts_with("dimensions:"))
        && let Some(size) = line.split_whitespace().nth(1).and_then(pair)
    {
        return Some(size);
    }

    let output = text("xrandr", &["--current"])?;
    let line = output.lines().find(|line| line.contains("current"))?;
    let after = line.split("current").nth(1)?;
    let numbers = after.split(',').next()?.replace(' ', "");

    pair(&numbers)
}

const AUD: [u8; 5] = [0, 0, 0, 1, 9];

/// Um quadro inteiro do pipe: do primeiro delimitador (`AUD`) até o seguinte, sem o
/// delimitador. Só devolve quando o quadro seguinte já começou — antes disso o quadro
/// atual pode ainda estar chegando.
fn take_access_unit(pending: &mut Vec<u8>) -> Option<Vec<u8>> {
    let first = find(pending, &AUD, 0)?;
    let second = find(pending, &AUD, first + AUD.len())?;
    let mut body = find(&pending[..second], &[0, 0, 1], first + AUD.len())?;

    // O código de início pode ter quatro bytes; o zero a mais fica com o quadro.
    if body > first + AUD.len() && pending[body - 1] == 0 {
        body -= 1;
    }

    let data = pending[body..second].to_vec();

    pending.drain(..second);

    Some(data)
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

fn has_idr(data: &[u8]) -> bool {
    data.windows(4)
        .any(|window| window[..3] == [0, 0, 1] && window[3] & 0x1f == 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_frames_on_the_delimiter_and_drops_it() {
        let mut pending = Vec::new();

        pending.extend([0, 0, 0, 1, 9, 0x10]);
        pending.extend([0, 0, 0, 1, 0x67, 1, 2]);
        pending.extend([0, 0, 1, 0x65, 3, 4]);

        assert!(take_access_unit(&mut pending).is_none(), "quadro ainda aberto");

        pending.extend([0, 0, 0, 1, 9, 0x30]);
        pending.extend([0, 0, 1, 0x41, 5]);

        let frame = take_access_unit(&mut pending).expect("keyframe fechado");

        assert_eq!(frame, [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x65, 3, 4]);
        assert!(has_idr(&frame));
        assert!(take_access_unit(&mut pending).is_none());

        pending.extend(AUD);
        let frame = take_access_unit(&mut pending).expect("quadro P fechado");

        assert_eq!(frame, [0, 0, 1, 0x41, 5]);
        assert!(! has_idr(&frame));
    }
}
