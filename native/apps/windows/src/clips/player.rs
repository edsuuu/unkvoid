//! O player da galeria: o `IMFMediaEngine` do Windows — o motor do `<video>` do antigo Edge —
//! desenhando numa janela filha posta sobre o retângulo do vídeo na janela do Slint.
//!
//! O Slint não toca vídeo. Decodificar à mão seria refazer o que o Windows já tem pronto:
//! H.264 e AAC pela placa, som sincronizado, pular para qualquer ponto. Aqui o Slint desenha
//! os controles e o Windows desenha o vídeo.

use std::path::Path;

use anyhow::Context;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFMediaEngine, IMFMediaEngineClassFactory,
    IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, MF_MEDIA_ENGINE_CALLBACK,
    MF_MEDIA_ENGINE_CREATEFLAGS, MF_MEDIA_ENGINE_PLAYBACK_HWND, MFCreateAttributes,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_STYLE, GetWindowLongPtrW, HWND_TOP,
    RegisterClassW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, WINDOW_EX_STYLE,
    WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
};
use windows::core::{BSTR, implement, w};

pub struct Player {
    engine: IMFMediaEngine,
    window: HWND,
}

impl Player {
    /// Abre o clipe tocando, numa janela filha de `parent`. Precisa do COM na thread.
    pub fn open(parent: HWND, path: &Path) -> anyhow::Result<Self> {
        unsafe {
            clips_engine::encoder::start_media_foundation()?;

            let instance = GetModuleHandleW(None)?;
            let class = w!("unkvoid-clips-video");

            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance.into(),
                lpszClassName: class,
                hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                ..Default::default()
            });

            // Sem isto a janela do Slint pinta por cima da filha a cada quadro da interface.
            let style = GetWindowLongPtrW(parent, GWL_STYLE);

            SetWindowLongPtrW(parent, GWL_STYLE, style | WS_CLIPCHILDREN.0 as isize);

            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                Some(parent),
                None,
                Some(instance.into()),
                None,
            )?;

            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)?;
            let mut attributes: Option<IMFAttributes> = None;

            MFCreateAttributes(&mut attributes, 2)?;

            let attributes = attributes.context("o Media Foundation não criou os atributos")?;
            let notify: IMFMediaEngineNotify = Notify.into();

            attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)?;
            attributes.SetUINT64(&MF_MEDIA_ENGINE_PLAYBACK_HWND, window.0 as u64)?;

            let engine = factory.CreateInstance(MF_MEDIA_ENGINE_CREATEFLAGS(0).0 as u32, &attributes)?;

            engine.SetSource(&BSTR::from(path.to_string_lossy().as_ref()))?;
            engine.Play()?;

            Ok(Self { engine, window })
        }
    }

    /// Põe o vídeo sobre o retângulo, em pixels físicos da área cliente da janela mãe.
    pub fn place(&self, x: i32, y: i32, width: i32, height: i32) {
        unsafe {
            let _ = SetWindowPos(self.window, Some(HWND_TOP), x, y, width.max(1), height.max(1), SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    pub fn toggle(&self) {
        unsafe {
            if self.engine.IsEnded().as_bool() {
                let _ = self.engine.SetCurrentTime(0.0);
                let _ = self.engine.Play();
            } else if self.engine.IsPaused().as_bool() {
                let _ = self.engine.Play();
            } else {
                let _ = self.engine.Pause();
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        unsafe { !self.engine.IsPaused().as_bool() && !self.engine.IsEnded().as_bool() }
    }

    /// Onde está e quanto dura, em segundos. A duração é `NaN` até o arquivo abrir.
    pub fn position(&self) -> (f64, f64) {
        unsafe { (self.engine.GetCurrentTime(), self.engine.GetDuration()) }
    }

    /// De 0 a 1.
    pub fn set_volume(&self, volume: f64) {
        unsafe {
            let _ = self.engine.SetVolume(volume.clamp(0.0, 1.0));
        }
    }

    pub fn seek(&self, fraction: f32) {
        let (_, duration) = self.position();

        if duration.is_finite() {
            unsafe {
                let _ = self.engine.SetCurrentTime(duration * f64::from(fraction.clamp(0.0, 1.0)));
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe {
            let _ = self.engine.Shutdown();
            let _ = DestroyWindow(self.window);
        }
    }
}

/// O motor exige quem ouça os eventos dele; o estado é lido por consulta, então nenhum
/// importa aqui.
#[implement(IMFMediaEngineNotify)]
struct Notify;

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, _event: u32, _first: usize, _second: u32) -> windows::core::Result<()> {
        Ok(())
    }
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
