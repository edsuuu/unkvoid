use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use screencapturekit::prelude::*;
use screencapturekit::cm::CMTime;
use screencapturekit::screenshot_manager::{CGImageExt, ImageFormat, SCScreenshotManager};

use crate::{
    AudioChunk, CaptureConfig, CaptureError, CaptureEvent, CaptureSource, Display, VideoFrame,
    Window,
};

/// Capture via ScreenCaptureKit. Requires macOS 13+ for video and system audio.
pub struct MacCapturer {
    stream: SCStream,
    frames: Arc<AtomicU64>,
    audio_chunks: Arc<AtomicU64>,
}

struct Sink<F: Fn(CaptureEvent) + Send + Sync + 'static> {
    on_event: Arc<F>,
    frames: Arc<AtomicU64>,
    audio_chunks: Arc<AtomicU64>,
    started_at: std::time::Instant,
}

impl<F: Fn(CaptureEvent) + Send + Sync + 'static> SCStreamOutputTrait for Sink<F> {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, kind: SCStreamOutputType) {
        let timestamp_ns = self.started_at.elapsed().as_nanos() as u64;

        match kind {
            SCStreamOutputType::Screen => {
                self.frames.fetch_add(1, Ordering::Relaxed);

                let (width, height) = frame_size(&sample);

                (self.on_event)(CaptureEvent::Video(VideoFrame {
                    width,
                    height,
                    timestamp_ns,
                    surface: sample.image_buffer().and_then(|buffer| buffer.io_surface()),
                }));
            }
            SCStreamOutputType::Audio => {
                self.audio_chunks.fetch_add(1, Ordering::Relaxed);

                let Some(samples) = interleave(&sample) else {
                    return;
                };

                (self.on_event)(CaptureEvent::Audio(AudioChunk {
                    sample_rate: 48_000,
                    channels: 2,
                    samples,
                }));
            }
            _ => {}
        }
    }
}

/// ScreenCaptureKit provides one float32 buffer per channel. Opus and WebRTC
/// require interleaved samples (L, R, L, R...), so conversion happens here.
fn interleave(sample: &CMSampleBuffer) -> Option<Vec<f32>> {
    let list = sample.audio_buffer_list()?;
    let channels = list.num_buffers();

    if channels == 0 {
        return None;
    }

    let planos: Vec<&[f32]> = (0..channels)
        .filter_map(|index| list.buffer(index))
        .map(|buffer| {
            let bytes = buffer.data();

            // SAFETY: ScreenCaptureKit is configured for float32, and the
            // AudioBufferList reporta o tamanho real em bytes.
            unsafe {
                std::slice::from_raw_parts(
                    bytes.as_ptr().cast::<f32>(),
                    bytes.len() / size_of::<f32>(),
                )
            }
        })
        .collect();

    let frames = planos.iter().map(|plano| plano.len()).min()?;
    let mut intercalado = Vec::with_capacity(frames * planos.len());

    for frame in 0..frames {
        for plano in &planos {
            intercalado.push(plano[frame]);
        }
    }

    Some(intercalado)
}

fn frame_size(sample: &CMSampleBuffer) -> (u32, u32) {
    sample
        .image_buffer()
        .map(|buffer| (buffer.width() as u32, buffer.height() as u32))
        .unwrap_or((0, 0))
}

impl MacCapturer {
    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        let content =
            SCShareableContent::get().map_err(|error| CaptureError::Platform(error.to_string()))?;

        Ok(content
            .displays()
            .into_iter()
            .map(|display| Display {
                id: display.display_id(),
                width: display.width(),
                height: display.height(),
            })
            .collect())
    }

    pub fn windows() -> Result<Vec<Window>, CaptureError> {
        let content =
            SCShareableContent::get().map_err(|error| CaptureError::Platform(error.to_string()))?;

        Ok(content
            .windows()
            .into_iter()
            .filter(|window| window.title().is_some_and(|title| !title.is_empty()))
            .map(|window| Window {
                id: u64::from(window.window_id()),
                title: window.title().unwrap_or_default(),
                application: window
                    .owning_application()
                    .map(|app| app.application_name())
                    .unwrap_or_default(),
            })
            .collect())
    }

    /// Miniatura de uma tela ou janela, em JPEG.
    ///
    /// Serve para a pessoa **ver** o que vai transmitir antes de transmitir. Um nome de
    /// janela não basta: "Terminal" e "Terminal" são dois, e escolher errado manda para
    /// a sala o que ela não queria mostrar.
    ///
    /// Pequena de propósito — é um preview, e gerar uma dúzia em tamanho real
    /// engasgaria a abertura do seletor.
    pub fn preview(source: CaptureSource) -> Result<Vec<u8>, CaptureError> {
        const WIDTH: u32 = 480;
        const HEIGHT: u32 = 270;

        let content =
            SCShareableContent::get().map_err(|error| CaptureError::Platform(error.to_string()))?;

        let filter = match source {
            CaptureSource::Window(id) => {
                let window = content
                    .windows()
                    .into_iter()
                    .find(|window| u64::from(window.window_id()) == id)
                    .ok_or(CaptureError::NoDisplay)?;

                SCContentFilter::create().with_window(&window).build()
            }
            other => {
                let displays = content.displays();

                let display = match other {
                    CaptureSource::Display(id) => {
                        displays.into_iter().find(|d| d.display_id() == id)
                    }
                    _ => displays.into_iter().next(),
                }
                .ok_or(CaptureError::NoDisplay)?;

                SCContentFilter::create().with_display(&display).build()
            }
        };

        let configuration = SCStreamConfiguration::new()
            .with_width(WIDTH)
            .with_height(HEIGHT)
            .with_pixel_format(PixelFormat::BGRA)
            .with_shows_cursor(false);

        let image = SCScreenshotManager::capture_image(&filter, &configuration)
            .map_err(|error| CaptureError::Platform(error.to_string()))?;

        // O macOS já sabe codificar JPEG; escrever um encoder aqui seria refazer o que
        // o sistema faz melhor. O arquivo é temporário e some logo em seguida.
        let caminho =
            std::env::temp_dir().join(format!("unkvoid-preview-{}.jpg", std::process::id()));
        let texto = caminho.to_string_lossy().to_string();

        image
            .save(&texto, ImageFormat::Jpeg(0.7))
            .map_err(|error| CaptureError::Platform(error.to_string()))?;

        let bytes =
            std::fs::read(&caminho).map_err(|error| CaptureError::Platform(error.to_string()))?;

        let _ = std::fs::remove_file(&caminho);

        Ok(bytes)
    }

    pub fn start<F>(config: &CaptureConfig, on_event: F) -> Result<Self, CaptureError>
    where
        F: Fn(CaptureEvent) + Send + Sync + 'static,
    {
        let content =
            SCShareableContent::get().map_err(|error| CaptureError::Platform(error.to_string()))?;

        // Uma janela específica em vez do monitor inteiro: quem escolheu compartilhar só
        // o jogo não pode ter o e-mail aparecendo junto.
        let filter = match config.source {
            CaptureSource::Window(id) => {
                let window = content
                    .windows()
                    .into_iter()
                    .find(|window| u64::from(window.window_id()) == id)
                    .ok_or(CaptureError::NoDisplay)?;

                SCContentFilter::create().with_window(&window).build()
            }
            source => {
                let displays = content.displays();

                let display = match source {
                    CaptureSource::Display(id) => displays
                        .into_iter()
                        .find(|display| display.display_id() == id),
                    _ => displays.into_iter().next(),
                }
                .ok_or(CaptureError::NoDisplay)?;

                SCContentFilter::create()
                    .with_display(&display)
                    .with_excluding_windows(&[])
                    .build()
            }
        };

        let (width, height) = config.quality.dimensions();

        let stream_config = SCStreamConfiguration::new()
            .with_width(width)
            .with_height(height)
            .with_pixel_format(PixelFormat::BGRA)
            .with_shows_cursor(config.show_cursor)
            .with_minimum_frame_interval(&CMTime::new(1, config.frame_rate as i32))
            .with_queue_depth(3)
            .with_captures_audio(config.capture_audio)
            // Our process's audio is excluded: this prevents sending back the voice
            // of someone in the call.
            .with_excludes_current_process_audio(CaptureConfig::EXCLUI_AUDIO_DO_APP)
            .with_sample_rate(48_000)
            .with_channel_count(2);

        let frames = Arc::new(AtomicU64::new(0));
        let audio_chunks = Arc::new(AtomicU64::new(0));

        let mut stream = SCStream::new(&filter, &stream_config);
        let on_event = Arc::new(on_event);
        let started_at = std::time::Instant::now();

        // Video and audio are separate ScreenCaptureKit outputs and each needs its
        // own handler. Registering only the screen handler silently omitted audio.
        for kind in [SCStreamOutputType::Screen, SCStreamOutputType::Audio] {
            if kind == SCStreamOutputType::Audio && !config.capture_audio {
                continue;
            }

            stream.add_output_handler(
                Sink {
                    on_event: on_event.clone(),
                    frames: frames.clone(),
                    audio_chunks: audio_chunks.clone(),
                    started_at,
                },
                kind,
            );
        }

        stream
            .start_capture()
            .map_err(|error| CaptureError::Platform(error.to_string()))?;

        Ok(Self {
            stream,
            frames,
            audio_chunks,
        })
    }

    pub fn frames_captured(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn audio_chunks_captured(&self) -> u64 {
        self.audio_chunks.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        self.stream
            .stop_capture()
            .map_err(|error| CaptureError::Platform(error.to_string()))
    }
}
