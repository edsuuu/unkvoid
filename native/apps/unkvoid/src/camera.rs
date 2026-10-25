//! A própria câmera. Um leitor só, aberto enquanto alguém precisa dele: o "Testar vídeo" de Voz
//! e vídeo, o "Vamos ligar a câmera?" e a câmera ligada na chamada. Cada quadro vai para a tela
//! e, com a câmera ligada, para o encoder pela sala.
//!
//! Só o Windows lê a webcam por aqui. No Linux a captura do núcleo abre e codifica a câmera
//! sozinha (`Room::open_captured_camera`), sem prévia; no macOS a câmera ainda não chegou ao app.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use core_app::room::Room;

use crate::watching::Picture;

/// Se este sistema mostra a própria câmera na tela.
pub const PREVIEWS: bool = cfg!(target_os = "windows");

/// Se este sistema liga a câmera na chamada.
pub const FILMS: bool = cfg!(any(target_os = "windows", target_os = "linux"));

/// O tamanho que se pede à webcam: o cartão da chamada é 720p, como o do Discord.
pub const SIZE: (u32, u32) = (1280, 720);
pub const FRAME_RATE: u32 = 30;

#[derive(Clone, Default)]
pub struct OwnCamera {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    opened: Mutex<Option<Opened>>,
    picture: Mutex<Option<Arc<Picture>>>,
    serial: AtomicU64,
    /// A sala para onde vai o quadro, com a câmera ligada na chamada.
    room: Mutex<Option<Arc<Room>>>,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
struct Opened {
    index: u32,
    #[cfg(target_os = "windows")]
    _reader: capture::CameraReader,
}

impl OwnCamera {
    #[cfg(not(target_os = "windows"))]
    pub fn open(&self, _index: u32, _context: &egui::Context) -> Result<(), capture::CaptureError> {
        Err(capture::CaptureError::Platform(
            "este sistema não mostra a própria câmera".into(),
        ))
    }

    /// Abre a câmera `index`, ou troca para ela. `context` acorda a janela a cada quadro.
    #[cfg(target_os = "windows")]
    pub fn open(&self, index: u32, context: &egui::Context) -> Result<(), capture::CaptureError> {
        if lock(&self.inner.opened).as_ref().is_some_and(|opened| opened.index == index) {
            return Ok(());
        }

        // Duas aberturas da mesma webcam brigam por ela: a velha sai antes.
        lock(&self.inner.opened).take();

        let inner = Arc::clone(&self.inner);
        let context = context.clone();
        let mut uploader: Option<capture::SurfaceUploader> = None;
        let reader = capture::CameraReader::open(index, SIZE.0, SIZE.1, FRAME_RATE, move |frame| {
            let room = lock(&inner.room).clone();

            if let Some(room) = room {
                if uploader.is_none() {
                    uploader = capture::SurfaceUploader::new()
                        .inspect_err(|failure| tracing::warn!(%failure, "câmera: a GPU não recebeu a webcam"))
                        .ok();
                }

                if let Some(surface) = uploader.as_mut().and_then(|uploader| uploader.upload(&frame).ok()) {
                    room.show(&surface, frame.timestamp_ns);
                }
            }

            let (width, height) = (frame.width, frame.height);
            let mut pixels = frame.bgra;

            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }

            *lock(&inner.picture) = Some(Arc::new(Picture { width, height, pixels }));
            inner.serial.fetch_add(1, Ordering::Relaxed);
            context.request_repaint();
        })?;

        *lock(&self.inner.opened) = Some(Opened { index, _reader: reader });

        Ok(())
    }

    /// Fecha a webcam: a luz dela apaga e a prévia some.
    pub fn close(&self) {
        let opened = lock(&self.inner.opened).take();

        // Juntar a thread da webcam espera o quadro que ela está lendo; a janela não espera.
        std::thread::spawn(move || drop(opened));
        lock(&self.inner.picture).take();
        self.inner.serial.fetch_add(1, Ordering::Relaxed);
    }

    /// O quadro mais novo e o número dele: a janela só refaz a textura quando o número muda.
    pub fn picture(&self) -> (u64, Option<Arc<Picture>>) {
        (self.inner.serial.load(Ordering::Relaxed), lock(&self.inner.picture).clone())
    }

    /// Manda os quadros para a sala (a câmera ligada na chamada), ou para de mandar.
    pub fn send_to(&self, room: Option<Arc<Room>>) {
        *lock(&self.inner.room) = room;
    }

    /// O tamanho do último quadro: a sala abre o encoder nele.
    pub fn size(&self) -> Option<(u32, u32)> {
        lock(&self.inner.picture).as_ref().map(|picture| (picture.width, picture.height))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
