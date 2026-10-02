//! Captura do monitor pelo Windows Graphics Capture, com o encoder dentro do callback.
//!
//! A textura do quadro só vale enquanto o callback roda, então a cópia para o encoder
//! acontece ali — e é só trabalho de GPU: cópia, blit e entregar ao MFT. O que custa CPU de
//! verdade, escrever no disco, sai por um canal para a thread do buffer: o callback nunca
//! espera o disco.
//!
//! A tela inteira, sempre: não há regra que pause a gravação quando um navegador abre. O que
//! aparece no monitor é o que vai para o clipe. Conteúdo com DRM por hardware (streaming de
//! filme com aceleração ligada) chega preto — é o Windows que entrega assim, e o app não
//! contorna isso.

use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context as _;
use capture::windows_duplication::Duplication;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11Texture2D};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::encoder::{Encoder, EncoderSettings};

const STATS_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
use crate::replay::{Record, RecordSink, Track};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoSettings {
    /// Posição na lista de `monitors()`; `None` é o monitor principal.
    pub monitor: Option<usize>,
    pub frame_rate: u32,
    pub bitrate: u32,
}

/// O que está gravando: o Graphics Capture, ou o Desktop Duplication no Windows que não tira a
/// borda amarela — a borda saía em todo clipe.
enum Running {
    Graphics(CaptureControl<Handler, anyhow::Error>),
    Duplication(Duplication),
}

pub struct ScreenCapture {
    control: Option<Running>,

    /// Largura e altura que o encoder está produzindo, empacotadas; zero até o primeiro quadro.
    frame_size: Arc<AtomicU64>,
}

impl ScreenCapture {
    pub fn start(settings: VideoSettings, sink: RecordSink) -> anyhow::Result<Self> {
        let monitor = match settings.monitor {
            // A lista é a do `EnumDisplayMonitors`, e o `from_index` da crate conta a partir
            // de 1 — o mesmo que o unkvoid descobriu.
            // O escolhido desligado ou desconectado: grava o principal, como a tela mostra.
            Some(index) => Monitor::from_index(index + 1).or_else(|_| Monitor::primary()),
            None => Monitor::primary(),
        }
        .context("o monitor escolhido não existe mais")?;

        let frame_size = Arc::new(AtomicU64::new(0));

        if Duplication::needed() {
            let mut handler = Handler::fresh(settings, sink.clone(), frame_size.clone());
            let started = Duplication::start(monitor, settings.frame_rate, true, move |frame| {
                match handler.frame(frame.texture, frame.context, (frame.width, frame.height), frame.timestamp_ns) {
                    Ok(()) => ControlFlow::Continue(()),
                    // Parar é o que faz o gravador religar a captura, como no Graphics Capture.
                    Err(error) => {
                        tracing::warn!(error = %error, "captura: o encoder recusou o quadro");

                        ControlFlow::Break(())
                    }
                }
            });

            match started {
                Ok(duplication) => return Ok(Self { control: Some(Running::Duplication(duplication)), frame_size }),
                Err(failure) => tracing::warn!(%failure, "captura: o Desktop Duplication não abriu, o replay vai com a borda"),
            }
        }

        let control = Handler::start_free_threaded(Settings::new(
            monitor,
            cursor_settings(),
            border_settings(),
            SecondaryWindowSettings::Default,
            update_interval(settings.frame_rate),
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            (settings, sink, frame_size.clone()),
        ))
        .map_err(|error| anyhow::anyhow!("o Windows recusou a captura da tela: {error}"))?;

        Ok(Self { control: Some(Running::Graphics(control)), frame_size })
    }

    /// O tamanho do vídeo que está sendo gravado, depois do primeiro quadro.
    pub fn frame_size(&self) -> Option<(u32, u32)> {
        let packed = self.frame_size.load(Ordering::Relaxed);

        (packed != 0).then_some(((packed >> 32) as u32, packed as u32))
    }

    /// A captura para sozinha quando o encoder falha ou o monitor some.
    pub fn is_running(&self) -> bool {
        match &self.control {
            Some(Running::Graphics(control)) => !control.is_finished(),
            Some(Running::Duplication(duplication)) => duplication.is_running(),
            None => false,
        }
    }

    pub fn stop(&mut self) {
        match self.control.take() {
            Some(Running::Graphics(control)) => {
                if let Err(error) = control.stop() {
                    tracing::warn!(error = %error, "captura: parou com erro");
                }
            }
            Some(Running::Duplication(mut duplication)) => duplication.stop(),
            None => {}
        }
    }
}

impl Drop for ScreenCapture {
    /// Sem isto a thread da captura continuava gravando depois de o dono sumir: nem a crate
    /// nem o controle dela param sozinhos.
    fn drop(&mut self) {
        self.stop();
    }
}

/// O tamanho do monitor escolhido (`None` é o principal), antes de a captura começar.
pub fn monitor_size(monitor: Option<usize>) -> Option<(u32, u32)> {
    let monitor = match monitor {
        Some(index) => Monitor::from_index(index + 1).or_else(|_| Monitor::primary()),
        None => Monitor::primary(),
    }
    .ok()?;

    Some((monitor.width().ok()?, monitor.height().ok()?))
}

pub struct MonitorInfo {
    /// A posição na lista, a mesma que `VideoSettings::monitor` guarda.
    pub index: usize,
    /// O nome do aparelho ("AW2525HM"), quando o Windows sabe.
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    /// O principal do Windows (Configurações > Tela > "Tornar este meu vídeo principal").
    pub primary: bool,
    /// O `HMONITOR`, para tirar a miniatura da tela.
    pub handle: isize,
}

pub fn monitors() -> anyhow::Result<Vec<MonitorInfo>> {
    let primary = Monitor::primary().ok().map(|monitor| monitor.as_raw_hmonitor() as isize);

    Ok(Monitor::enumerate()?
        .into_iter()
        .enumerate()
        .map(|(index, monitor)| MonitorInfo {
            index,
            name: monitor.name().unwrap_or_default(),
            width: monitor.width().unwrap_or(0),
            height: monitor.height().unwrap_or(0),
            refresh_rate: monitor.refresh_rate().unwrap_or(0),
            primary: primary == Some(monitor.as_raw_hmonitor() as isize),
            handle: monitor.as_raw_hmonitor() as isize,
        })
        .collect())
}

struct Handler {
    settings: VideoSettings,
    sink: RecordSink,
    frame_size: Arc<AtomicU64>,

    /// Nasce no primeiro quadro, quando o tamanho real da origem é conhecido.
    encoder: Option<Encoder>,

    /// Quadros que chegaram desde `counted_since`, para o log dizer se a captura está
    /// recebendo o que devia: é a primeira pergunta quando um clipe sai picotado.
    frames: u64,
    counted_since: std::time::Instant,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = (VideoSettings, RecordSink, Arc<AtomicU64>);
    type Error = anyhow::Error;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (settings, sink, frame_size) = context.flags;

        Ok(Self::fresh(settings, sink, frame_size))
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let timestamp_ns = crate::clock::from_hundred_nanoseconds(frame.timestamp()?.Duration);

        self.frame(frame.as_raw_texture(), frame.device_context(), (frame.width(), frame.height()), timestamp_ns)
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        tracing::warn!("captura: o Windows encerrou a captura do monitor");

        Ok(())
    }
}

impl Handler {
    fn fresh(settings: VideoSettings, sink: RecordSink, frame_size: Arc<AtomicU64>) -> Self {
        Self { settings, sink, frame_size, encoder: None, frames: 0, counted_since: std::time::Instant::now() }
    }

    /// Um quadro do monitor, de qualquer uma das duas capturas, para o encoder.
    fn frame(
        &mut self,
        texture: &ID3D11Texture2D,
        context: &ID3D11DeviceContext,
        (width, height): (u32, u32),
        timestamp_ns: u64,
    ) -> anyhow::Result<()> {
        self.frames += 1;

        if self.counted_since.elapsed() >= STATS_INTERVAL {
            tracing::info!(frames = self.frames, seconds = STATS_INTERVAL.as_secs(), "captura: quadros recebidos");
            self.frames = 0;
            self.counted_since = std::time::Instant::now();
        }

        let encoder = match &mut self.encoder {
            Some(encoder) => encoder,
            None => {
                crate::recorder::join_multimedia_task("Capture");

                // NV12 guarda a cor em blocos de 2×2: largura ou altura ímpar é recusada.
                let (width, height) = (width & !1, height & !1);

                self.frame_size.store((u64::from(width) << 32) | u64::from(height), Ordering::Relaxed);
                self.encoder.insert(Encoder::new(EncoderSettings {
                    width,
                    height,
                    frame_rate: self.settings.frame_rate,
                    bitrate: self.settings.bitrate,
                })?)
            }
        };

        for encoded in encoder.encode(texture, context, timestamp_ns)? {
            self.sink.push(Record {
                track: Track::Video,
                keyframe: encoded.keyframe,
                timestamp_ns: encoded.timestamp_ns,
                data: encoded.data,
            });
        }

        Ok(())
    }
}

/// A crate recusa a captura inteira se receber uma opção que este Windows não tem; o que
/// faltar fica no padrão do sistema.
fn cursor_settings() -> CursorCaptureSettings {
    match GraphicsCaptureApi::is_cursor_settings_supported() {
        Ok(true) => CursorCaptureSettings::WithCursor,
        _ => CursorCaptureSettings::Default,
    }
}

/// A borda amarela só dá para tirar do Windows 11 em diante; no 10 o replay só passa por aqui
/// quando o Desktop Duplication não abre.
fn border_settings() -> DrawBorderSettings {
    if GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false) {
        DrawBorderSettings::WithoutBorder
    } else {
        DrawBorderSettings::Default
    }
}

/// O teto de quadros na origem (Windows 11 24H2 em diante): pedir 60 e receber 240 de um
/// monitor rápido faria o encoder jogar três quartos fora depois de o Windows já ter pago a
/// cópia de cada um.
fn update_interval(frame_rate: u32) -> MinimumUpdateIntervalSettings {
    if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) {
        MinimumUpdateIntervalSettings::Custom(std::time::Duration::from_secs_f64(1.0 / f64::from(frame_rate.max(1))))
    } else {
        MinimumUpdateIntervalSettings::Default
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::replay::ReplayBuffer;

    /// Na máquina de verdade, pelo Desktop Duplication — `UNKVOID_DUPLICATION=on` força o
    /// caminho do Windows 10 num Windows 11: o encoder recebe os quadros e o vídeo ganha tamanho.
    #[test]
    #[ignore = "precisa de um monitor e de um encoder de hardware"]
    fn the_replay_records_through_the_desktop_duplication() {
        let folder = std::env::temp_dir().join(format!("unkvoid-clips-teste-{}", std::process::id()));
        let buffer = ReplayBuffer::start(folder.clone(), Duration::from_secs(30)).expect("buffer");
        let mut capture = ScreenCapture::start(VideoSettings { monitor: None, frame_rate: 30, bitrate: 5_000_000 }, buffer.sink())
            .expect("captura");

        assert_eq!(
            matches!(capture.control, Some(Running::Duplication(_))),
            Duplication::needed(),
            "o replay não foi pelo caminho deste Windows"
        );

        std::thread::sleep(Duration::from_secs(2));

        assert!(capture.is_running(), "a captura parou sozinha");
        assert!(capture.frame_size().is_some(), "nenhum quadro chegou ao encoder");

        capture.stop();
        drop(buffer);

        let _ = std::fs::remove_dir_all(folder);
    }
}
