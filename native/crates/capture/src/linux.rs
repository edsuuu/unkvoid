//! Captura no Linux: X11 pelo GStreamer, já codificada.
//!
//! Ligar a biblioteca do GStreamer ao binário exigiria as `-dev` no build e as `.so`
//! certas em cada máquina. Então o app fala com o `gst-launch-1.0` como processo:
//! `ximagesrc` lê a tela, o encoder da placa que abrir (`LinuxCapturer::video_encoder`)
//! ou o `x264enc` comprime, e o H.264 (Annex-B) chega por um pipe. O `.deb` já exige os
//! plugins; o `gstreamer1.0-tools` é a única dependência a mais.
//!
//! O que sai daqui NÃO é buffer de GPU: é o quadro pronto, e `PlatformEncoder` no
//! Linux só o repassa. É o jeito de encaixar no fluxo dos outros sistemas sem mexer
//! no `broadcast.rs`.
//!
//! ponytail: só X11 (`ximagesrc`). Numa sessão Wayland pura o `DISPLAY` não existe e a
//! lista de telas sai vazia; o caminho é `pipewiresrc` via portal quando alguém pedir.
//! Sem lista de janelas ainda pelo mesmo motivo.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

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
    video: Option<Child>,
    audio: Option<Child>,
    frames: Arc<AtomicU64>,
    audio_chunks: Arc<AtomicU64>,
    /// A última linha de erro do gst de vídeo. É o que aparece no app quando a captura
    /// não gera quadro nenhum — sem isto o diagnóstico culpava a rede.
    error: Arc<Mutex<Option<String>>>,
}

impl LinuxCapturer {
    pub fn preview(source: CaptureSource) -> Result<Vec<u8>, CaptureError> {
        if std::env::var_os("DISPLAY").is_none() {
            return Ok(Vec::new());
        }

        let pipeline = format!(
            "ximagesrc use-damage=false num-buffers=1 {} ! videoconvert ! videoscale              ! video/x-raw,width=320,pixel-aspect-ratio=1/1 ! jpegenc ! fdsink fd=1",
            region(source).map(|monitor| monitor.area()).unwrap_or_default()
        );

        let output = Command::new("gst-launch-1.0")
            .arg("-q")
            .args(pipeline.split_whitespace())
            .stderr(Stdio::null())
            .output();

        Ok(output.map(|output| output.stdout).unwrap_or_default())
    }

    /// Um item por monitor, o principal primeiro. Sem `xrandr` fica a tela do X
    /// inteira, que com dois monitores é os dois lado a lado.
    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        if std::env::var_os("DISPLAY").is_none() {
            return Ok(Vec::new());
        }

        let monitors = monitors();

        if monitors.is_empty() {
            let (width, height) = screen_size().unwrap_or((0, 0));

            return Ok(vec![Display { id: 1, width, height }]);
        }

        Ok(monitors
            .iter()
            .enumerate()
            .map(|(index, monitor)| Display {
                id: index as u32 + 1,
                width: monitor.width,
                height: monitor.height,
            })
            .collect())
    }

    pub fn windows() -> Result<Vec<Window>, CaptureError> {
        Ok(Vec::new())
    }

    /// As câmeras: `(caminho, nome)` de cada `/dev/video*`, pelo nome que o driver dá.
    ///
    /// A uvcvideo cria um nó de metadados ao lado do de captura, com o mesmo nome e o
    /// número seguinte; o de captura é o menor. Por isso a ordem é numérica e o nome
    /// repetido fica com o primeiro índice.
    pub fn cameras() -> Vec<(String, String)> {
        let found = std::fs::read_dir("/sys/class/video4linux")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let node = entry.file_name().into_string().ok()?;
                let index: u32 = node.strip_prefix("video")?.parse().ok()?;
                let name = std::fs::read_to_string(entry.path().join("name")).ok()?;

                Some((index, name.trim().to_string()))
            });

        dedupe_cameras(found)
    }

    /// O tamanho da origem, para a altura da saída seguir a proporção dela.
    pub fn source_size(source: CaptureSource) -> Result<(u32, u32), CaptureError> {
        Ok(match source {
            CaptureSource::Camera(_) => CAMERA_SIZE,
            CaptureSource::Microphone => (0, 0),
            _ => region(source)
                .map(|monitor| (monitor.width, monitor.height))
                .or_else(screen_size)
                .ok_or(CaptureError::NoDisplay)?,
        })
    }

    /// Descobre o que o GStreamer desta máquina sabe, fora de qualquer cadeado: a
    /// primeira pergunta abre um `gst-inspect`, e as seguintes lêem o cache.
    pub fn warm_up() {
        has_webrtcdsp();
        Self::video_encoder();
    }

    /// O encoder de H.264 desta máquina: o primeiro de `HARDWARE_H264_ENCODERS` que abre de
    /// verdade, ou o `x264enc`.
    ///
    /// Aparecer no `gst-inspect` não basta: o plugin da NVIDIA ou do VA-API vem instalado
    /// em máquina sem a placa, e só abrir o device diz. Cada candidato codifica um quadro
    /// de teste com o mesmo trecho de pipeline que a transmissão vai usar, uma vez por
    /// processo.
    pub fn video_encoder() -> &'static str {
        static CHOSEN: OnceLock<&'static str> = OnceLock::new();

        CHOSEN.get_or_init(|| {
            // `UNKVOID_ENCODER=cpu` pula a placa: sem isto o x264 só roda em máquina sem
            // placa, e ninguém que desenvolve tem uma à mão.
            let forced_cpu = std::env::var("UNKVOID_ENCODER").is_ok_and(|value| value == "cpu");
            let chosen = HARDWARE_H264_ENCODERS
                .into_iter()
                .find(|element| !forced_cpu && encoder_opens(element))
                .unwrap_or("x264enc");

            tracing::info!(encoder = chosen, "captura: encoder de H.264 escolhido");

            chosen
        })
    }

    pub fn start<F>(config: &CaptureConfig, on_event: F) -> Result<Self, CaptureError>
    where
        F: Fn(CaptureEvent) + Send + Sync + 'static,
    {
        let on_event: Arc<dyn Fn(CaptureEvent) + Send + Sync> = Arc::new(on_event);
        let frames = Arc::new(AtomicU64::new(0));
        let audio_chunks = Arc::new(AtomicU64::new(0));
        let error = Arc::new(Mutex::new(None));

        // Microfone: só áudio, com a mesma forma da tela para o app não saber a diferença.
        if config.source == CaptureSource::Microphone {
            let mut audio = launch(&microphone_pipeline(), true)?;

            watch_stderr(&mut audio, Arc::clone(&error));
            read_audio(&mut audio, Arc::clone(&audio_chunks), on_event);

            return Ok(Self { video: None, audio: Some(audio), frames, audio_chunks, error });
        }

        // Câmera: só vídeo, pequeno, já em H.264 como a tela.
        if let CaptureSource::Camera(index) = config.source {
            let (width, height) = CAMERA_SIZE;
            let mut video = launch(&camera_pipeline(index), true)?;

            watch_stderr(&mut video, Arc::clone(&error));
            read_video(&mut video, width, height, Arc::clone(&frames), on_event);

            return Ok(Self { video: Some(video), audio: None, frames, audio_chunks, error });
        }

        if std::env::var_os("DISPLAY").is_none() {
            return Err(CaptureError::NoDisplay);
        }

        let (width, height) = config.quality.fit(Self::source_size(config.source)?);
        let frame_rate = config.frame_rate.clamp(1, 60);

        // Os mesmos tetos do `EncoderConfig`, em kbit/s, porque aqui o encoder é o x264.
        let bitrate = match config.quality {
            Quality::Hd720 => 5_000,
            Quality::Hd1080 => 10_000,
            Quality::Qhd1440 => 20_000,
            Quality::Uhd2160 => 40_000,
        } * frame_rate
            / 60;

        // ponytail: sem pedido de keyframe por fora; um a cada segundo é o que quem entra
        // na sala espera no pior caso.
        let (format, encoder) = encoder_tail(Self::video_encoder(), frame_rate, bitrate);
        let pipeline = format!(
            "ximagesrc use-damage=false show-pointer={} {} ! video/x-raw,framerate={frame_rate}/1 \
             ! videoconvert ! videoscale ! video/x-raw,format={format},colorimetry=bt709,width={width},pixel-aspect-ratio=1/1 \
             ! {encoder}",
            config.show_cursor,
            region(config.source).map(|monitor| monitor.area()).unwrap_or_default()
        );

        let mut video = launch(&pipeline, true)?;

        watch_stderr(&mut video, Arc::clone(&error));
        read_video(&mut video, width, height, Arc::clone(&frames), Arc::clone(&on_event));

        let audio = if config.capture_audio {
            // O monitor da saída padrão é o som do sistema inteiro. Filtrar por app
            // (`MUTED_APPS`) não existe aqui.
            match launch(
                &format!("pulsesrc device=@DEFAULT_MONITOR@ ! {AUDIO_TAIL}"),
                false,
            ) {
                Ok(mut child) => {
                    read_audio(&mut child, Arc::clone(&audio_chunks), on_event);

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

        Ok(Self { video: Some(video), audio, frames, audio_chunks, error })
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn frames_captured(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn audio_chunks_captured(&self) -> u64 {
        self.audio_chunks.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        for child in [self.video.as_mut(), self.audio.as_mut()].into_iter().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }

        Ok(())
    }
}

impl Drop for LinuxCapturer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// A câmera sobe pequena: é um cartão ao lado da tela, não a tela.
const CAMERA_SIZE: (u32, u32) = (640, 360);

/// O fim de todo pipeline de áudio: 48 kHz estéreo em `f32`, como o `AudioEncoder` quer.
const AUDIO_TAIL: &str = "audioconvert ! audioresample \
    ! audio/x-raw,format=F32LE,rate=48000,channels=2,layout=interleaved ! fdsink fd=1 sync=false";

/// O microfone padrão, limpo pelo `webrtcdsp` quando a distro o tem (plugins bad).
///
/// ponytail: sem `webrtcechoprobe` o cancelamento de eco não tem o que cancelar — o som
/// dos outros toca em outro processo (`watch.rs`). Ficam a supressão de ruído e o ganho.
fn microphone_pipeline() -> String {
    let cleanup = if has_webrtcdsp() {
        "! audioconvert ! audio/x-raw,format=S16LE,rate=48000,channels=2,layout=interleaved \
         ! webrtcdsp echo-cancel=true noise-suppression=true gain-control=true "
    } else {
        ""
    };

    format!("pulsesrc device=@DEFAULT_SOURCE@ {cleanup}! {AUDIO_TAIL}")
}

/// O encoder da tela, mas em 640x360 a 30 fps e 800 kbit/s: um cartão pequeno não
/// precisa de mais, e é banda que a tela de alguém está usando.
fn camera_pipeline(index: u32) -> String {
    let (width, height) = CAMERA_SIZE;
    let (format, encoder) = encoder_tail(LinuxCapturer::video_encoder(), 30, 800);

    format!(
        "v4l2src device=/dev/video{index} ! videoconvert ! videoscale ! videorate \
         ! video/x-raw,format={format},colorimetry=bt709,width={width},height={height},framerate=30/1 \
         ! {encoder}"
    )
}

/// Os encoders de H.264 da placa, na ordem em que são tentados: NVIDIA, VA-API novo (Intel
/// e AMD, integrada inclusive) e o VA-API antigo das distros que ainda não têm o novo.
const HARDWARE_H264_ENCODERS: [&str; 3] = ["nvh264enc", "vah264enc", "vaapih264enc"];

/// Quanto a sondagem espera por um encoder. Um elemento que não existe falha em
/// milissegundos; um driver que pendura na abertura não pode segurar a transmissão.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// O formato cru que o encoder quer e o pipeline dele até o pipe.
///
/// Todos terminam iguais: H.264 byte-stream em constrained baseline (o `profile-level-id`
/// que o servidor anuncia), com AUD separando os quadros e SPS/PPS na frente de cada IDR —
/// é disso que `take_access_unit` e quem entra no meio dependem. Quem garante as duas
/// últimas é o `h264parse`: ele insere o AUD que falta e repete SPS/PPS por IDR
/// (`config-interval=-1`), o que o x264 fazia sozinho e os de placa nem sempre fazem.
///
/// ponytail: a conversão para NV12/I420 continua no `videoconvert`, na CPU, para todos;
/// `cudaconvert`/`vapostproc` a levariam para a placa quando o custo aparecer.
fn encoder_tail(element: &str, key_interval: u32, bitrate: u32) -> (&'static str, String) {
    let (format, encoder) = match element {
        "nvh264enc" => (
            "NV12",
            format!("nvh264enc preset=low-latency-hp zerolatency=true bframes=0 gop-size={key_interval} bitrate={bitrate}"),
        ),
        "vah264enc" => (
            "NV12",
            format!("vah264enc rate-control=cbr b-frames=0 cabac=false dct8x8=false key-int-max={key_interval} bitrate={bitrate}"),
        ),
        "vaapih264enc" => (
            "NV12",
            format!("vaapih264enc rate-control=cbr max-bframes=0 keyframe-period={key_interval} bitrate={bitrate}"),
        ),
        // `vbv-buf-capacity=100` (ms) é o que segura o pico de um keyframe dentro de um
        // décimo de segundo de banda, em vez de um segundo inteiro.
        _ => (
            "I420",
            format!(
                "x264enc tune=zerolatency speed-preset=ultrafast byte-stream=true aud=true \
                 key-int-max={key_interval} bitrate={bitrate} vbv-buf-capacity=100 threads=0"
            ),
        ),
    };

    (
        format,
        format!(
            "{encoder} ! h264parse config-interval=-1 \
             ! video/x-h264,stream-format=byte-stream,alignment=au,profile=constrained-baseline \
             ! fdsink fd=1 sync=false"
        ),
    )
}

/// Se o encoder abre nesta máquina: um quadro de teste pelo mesmo trecho da transmissão.
fn encoder_opens(element: &str) -> bool {
    let (format, encoder) = encoder_tail(element, 30, 1_000);
    let pipeline = format!(
        "videotestsrc num-buffers=1 ! videoconvert ! video/x-raw,format={format},width=640,height=360 ! {encoder}"
    );

    let Ok(mut child) = Command::new("gst-launch-1.0")
        .arg("-q")
        .args(pipeline.split_whitespace())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };

    let started = Instant::now();

    while started.elapsed() < PROBE_TIMEOUT {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }

    tracing::warn!(encoder = element, "captura: a sondagem do encoder não terminou a tempo");

    let _ = child.kill();
    let _ = child.wait();

    false
}

/// Se o `webrtcdsp` existe. Perguntado uma vez por processo: o `gst-inspect` leva
/// dezenas de milissegundos, e ligar o microfone acontecia com a sessão trancada.
fn has_webrtcdsp() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();

    *FOUND.get_or_init(|| {
        Command::new("gst-inspect-1.0")
            .arg("webrtcdsp")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Ordem numérica e um nó por nome, o de menor índice.
fn dedupe_cameras(found: impl Iterator<Item = (u32, String)>) -> Vec<(String, String)> {
    let mut found: Vec<(u32, String)> = found.collect();

    found.sort();
    found.dedup_by(|later, earlier| later.1 == earlier.1);

    found
        .into_iter()
        .map(|(index, name)| (format!("/dev/video{index}"), name))
        .collect()
}

/// Guarda a última linha de erro do gst: é o que aparece no app quando a captura não
/// gera quadro nenhum — sem isto o diagnóstico culpava a rede.
fn watch_stderr(child: &mut Child, error: Arc<Mutex<Option<String>>>) {
    let Some(stderr) = child.stderr.take() else {
        return;
    };

    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            tracing::warn!(line = %line, "gst");

            if (line.starts_with("ERROR") || line.contains("rror"))
                && let Ok(mut slot) = error.lock()
            {
                *slot = Some(line);
            }
        }
    });
}

/// O H.264 do pipe, um quadro por vez, para o callback.
fn read_video(
    child: &mut Child,
    width: u32,
    height: u32,
    frames: Arc<AtomicU64>,
    on_event: Arc<dyn Fn(CaptureEvent) + Send + Sync>,
) {
    let mut stdout = child.stdout.take().expect("stdout piped");

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
                frames.fetch_add(1, Ordering::Relaxed);

                on_event(CaptureEvent::Video(VideoFrame {
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
}

/// O áudio do pipe, em blocos de 20 ms, para o callback.
fn read_audio(
    child: &mut Child,
    chunks: Arc<AtomicU64>,
    on_event: Arc<dyn Fn(CaptureEvent) + Send + Sync>,
) {
    let mut stdout = child.stdout.take().expect("stdout piped");

    std::thread::spawn(move || {
        let mut block = vec![0_u8; AUDIO_BLOCK_BYTES];

        while stdout.read_exact(&mut block).is_ok() {
            chunks.fetch_add(1, Ordering::Relaxed);

            on_event(CaptureEvent::Audio(AudioChunk {
                sample_rate: 48_000,
                channels: 2,
                samples: block.as_chunks::<4>().0.iter().map(|bytes| f32::from_le_bytes(*bytes)).collect(),
            }));
        }

        tracing::warn!("captura: o gst-launch de áudio terminou (sem PulseAudio/PipeWire?)");
    });
}

fn launch(pipeline: &str, keep_stderr: bool) -> Result<Child, CaptureError> {
    Command::new("gst-launch-1.0")
        .arg("-q")
        .args(pipeline.split_whitespace())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(if keep_stderr { Stdio::piped() } else { Stdio::inherit() })
        .spawn()
        .map_err(|error| {
            CaptureError::Platform(format!(
                "gst-launch-1.0 não abriu ({error}); instale gstreamer1.0-tools e os plugins good/ugly"
            ))
        })
}

/// Um monitor como o `xrandr` o descreve: tamanho e posição dentro da tela do X.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Monitor {
    width: u32,
    height: u32,
    x: u32,
    y: u32,
}

impl Monitor {
    /// O recorte para o `ximagesrc`. As bordas são inclusivas.
    fn area(self) -> String {
        format!(
            "startx={} starty={} endx={} endy={}",
            self.x,
            self.y,
            self.x + self.width - 1,
            self.y + self.height - 1
        )
    }
}

/// Os monitores ligados, o principal primeiro. Dois monitores são UMA tela para o X;
/// sem isto a captura mandava os dois lado a lado, espremidos em 16:9.
fn monitors() -> Vec<Monitor> {
    let Some(output) = text("xrandr", &["--current"]) else {
        return Vec::new();
    };

    parse_monitors(&output)
}

fn parse_monitors(xrandr: &str) -> Vec<Monitor> {
    let mut found: Vec<(bool, Monitor)> = xrandr
        .lines()
        .filter(|line| line.contains(" connected "))
        .filter_map(|line| {
            let primary = line.contains(" primary ");
            let geometry = line.split_whitespace().find(|word| {
                word.contains('x') && word.matches('+').count() == 2
            })?;
            let (size, offset) = geometry.split_once('+')?;
            let (width, height) = size.split_once('x')?;
            let (x, y) = offset.split_once('+')?;

            Some((
                primary,
                Monitor {
                    width: width.parse().ok()?,
                    height: height.parse().ok()?,
                    x: x.parse().ok()?,
                    y: y.parse().ok()?,
                },
            ))
        })
        .collect();

    found.sort_by_key(|(primary, _)| ! primary);

    found.into_iter().map(|(_, monitor)| monitor).collect()
}

/// O monitor que uma escolha do seletor quer dizer. `None` é a tela do X inteira.
fn region(source: CaptureSource) -> Option<Monitor> {
    let monitors = monitors();

    match source {
        CaptureSource::Display(id) => monitors.get(id.checked_sub(1)? as usize).copied(),
        CaptureSource::PrimaryDisplay => monitors.first().copied(),
        CaptureSource::Window(_) | CaptureSource::Camera(_) | CaptureSource::Microphone => None,
    }
}

fn text(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Tamanho da tela pelo X, para o seletor mostrar. Sem `xdpyinfo` nem `xrandr` fica
/// sem número — a captura em si não depende disto.
fn screen_size() -> Option<(u32, u32)> {
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
    fn two_monitors_become_two_displays_primary_first() {
        let xrandr = "Screen 0: minimum 320 x 200, current 3840 x 1080, maximum 16384 x 16384\n\
            DP-1 connected 1920x1080+1920+0 (normal left inverted right x axis y axis) 527mm x 296mm\n\
            HDMI-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 527mm x 296mm\n\
            DP-2 disconnected (normal left inverted right x axis y axis)\n\
               1920x1080     60.00*+\n";

        let monitors = parse_monitors(xrandr);

        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0], Monitor { width: 1920, height: 1080, x: 0, y: 0 });
        assert_eq!(monitors[1].x, 1920);
        assert_eq!(monitors[1].area(), "startx=1920 starty=0 endx=3839 endy=1079");
    }

    #[test]
    fn every_encoder_ends_in_the_same_byte_stream() {
        for element in HARDWARE_H264_ENCODERS.into_iter().chain(["x264enc"]) {
            let (format, tail) = encoder_tail(element, 60, 5_000);

            assert!(tail.starts_with(element), "{tail}");
            assert!(["NV12", "I420"].contains(&format), "{element}: {format}");
            assert!(tail.contains("=60 ") && tail.contains("bitrate=5000"), "{element}: keyframe por segundo e a taxa: {tail}");
            assert!(
                tail.split_whitespace().collect::<Vec<_>>().join(" ").ends_with(
                    "! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au,profile=constrained-baseline ! fdsink fd=1 sync=false"
                ),
                "{element}: sem o h264parse o pipe perde o AUD ou o SPS/PPS por IDR: {tail}"
            );
        }
    }

    #[test]
    fn cameras_come_in_numeric_order_and_the_metadata_node_is_dropped() {
        let found = [
            (10, "Webcam B".to_string()),
            (2, "Webcam A".to_string()),
            (3, "Webcam A".to_string()),
            (11, "Webcam B".to_string()),
        ];

        assert_eq!(
            dedupe_cameras(found.into_iter()),
            [
                ("/dev/video2".to_string(), "Webcam A".to_string()),
                ("/dev/video10".to_string(), "Webcam B".to_string()),
            ]
        );
    }

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
