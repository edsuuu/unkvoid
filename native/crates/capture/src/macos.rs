use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use screencapturekit::prelude::*;

use crate::{AudioChunk, CaptureConfig, CaptureError, CaptureEvent, Display, VideoFrame, Window};

/// Captura via ScreenCaptureKit. Exige macOS 13+ para vídeo e áudio de sistema.
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

                (self.on_event)(CaptureEvent::Audio(AudioChunk {
                    sample_rate: 48_000,
                    channels: 2,
                    frames: sample.num_samples().max(0) as usize,
                }));
            }
            _ => {}
        }
    }
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
                id: window.window_id(),
                title: window.title().unwrap_or_default(),
                application: window
                    .owning_application()
                    .map(|app| app.application_name())
                    .unwrap_or_default(),
            })
            .collect())
    }

    pub fn start<F>(config: &CaptureConfig, on_event: F) -> Result<Self, CaptureError>
    where
        F: Fn(CaptureEvent) + Send + Sync + 'static,
    {
        let content =
            SCShareableContent::get().map_err(|error| CaptureError::Platform(error.to_string()))?;
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or(CaptureError::NoDisplay)?;

        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build();

        let (width, height) = config.quality.dimensions();

        let stream_config = SCStreamConfiguration::new()
            .with_width(width)
            .with_height(height)
            .with_pixel_format(PixelFormat::BGRA)
            .with_shows_cursor(config.show_cursor)
            .with_captures_audio(config.capture_audio)
            .with_sample_rate(48_000)
            .with_channel_count(2);

        let frames = Arc::new(AtomicU64::new(0));
        let audio_chunks = Arc::new(AtomicU64::new(0));

        let mut stream = SCStream::new(&filter, &stream_config);
        let on_event = Arc::new(on_event);
        let started_at = std::time::Instant::now();

        // Vídeo e áudio são saídas distintas do ScreenCaptureKit e cada uma precisa
        // do seu handler. Registrar só a de tela deixava o áudio de fora em silêncio.
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
