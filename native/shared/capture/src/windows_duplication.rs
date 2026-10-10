//! A tela inteira pelo Desktop Duplication do DXGI, no Windows que não tira a borda amarela.
//!
//! O Graphics Capture pinta a borda amarela em volta do que captura, e ela aparecia para quem
//! assistia e nos clipes. Tirá-la é uma opção que só existe do Windows 11 em diante (o app já
//! pede, em `allow_borderless_capture`); no Windows 10 o pedido é recusado. O Desktop
//! Duplication copia o monitor sem borda nenhuma, e o quadro também nasce na GPU: continua
//! indo direto ao encoder da placa, sem descer à memória do processador.
//!
//! O que ele não faz: janela avulsa — é sempre o monitor inteiro, então janela no Windows 10
//! segue pelo Graphics Capture, com a borda — e o cursor, que o Windows entrega à parte. O
//! cursor é desenhado aqui, pelo GDI do próprio Windows numa cópia do quadro que continua na
//! GPU: sai o mesmo desenho da tela, inclusive o cursor de texto que inverte o fundo.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ::windows::Win32::Foundation::POINT;
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_GDI_COMPATIBLE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use ::windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use ::windows::Win32::Graphics::Dxgi::IDXGISurface1;
use ::windows::Win32::Graphics::Gdi::DeleteObject;
use ::windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo, HCURSOR, HICON, ICONINFO,
};
use ::windows::core::Interface;
use windows_capture::dxgi_duplication_api::{DxgiDuplicationApi, DxgiDuplicationFormat, Error as DuplicationError};
use windows_capture::encoder::ImageFormat;
use windows_capture::graphics_capture_api::GraphicsCaptureApi;
use windows_capture::monitor::Monitor;

/// Quanto se espera um quadro novo antes de olhar se é hora de parar. Tela parada não manda
/// quadro nenhum, e sem prazo a thread não ouviria o pedido de parar.
const WAIT_MS: u32 = 100;

/// O intervalo entre duas tentativas de retomar a duplicação perdida. Ela cai na troca de
/// resolução, no aviso do UAC e na tela de bloqueio, e volta sozinha quando a área de trabalho
/// volta; antes disso o Windows recusa.
const RETRY: Duration = Duration::from_millis(200);

/// Quanto se espera a duplicação abrir antes de desistir dela e voltar ao Graphics Capture.
const OPENING: Duration = Duration::from_secs(5);

/// Um quadro do monitor, já com o cursor, na GPU. A textura é da duplicação e é reaproveitada
/// no quadro seguinte: quem recebe copia antes de devolver, como no Graphics Capture.
pub struct DuplicatedFrame<'a> {
    pub texture: &'a ID3D11Texture2D,
    pub device: &'a ID3D11Device,
    pub context: &'a ID3D11DeviceContext,
    pub width: u32,
    pub height: u32,
    /// No relógio do QPC, em nanossegundos: o mesmo que marca o som do sistema.
    pub timestamp_ns: u64,
}

pub struct Duplication {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Duplication {
    /// Este Windows não deixa tirar a borda do Graphics Capture: o caso do Windows 10.
    ///
    /// `UNKVOID_DUPLICATION=on` força este caminho num Windows 11, que é onde ele é testado: o
    /// Windows 10 não está na mesa de quem desenvolve.
    pub fn needed() -> bool {
        std::env::var("UNKVOID_DUPLICATION").is_ok_and(|value| value == "on")
            || !GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false)
    }

    /// Começa a duplicar o monitor e entrega cada quadro a `on_frame`, na thread dela, no
    /// máximo `frame_rate` por segundo. Só volta depois de o Windows aceitar: monitor que não
    /// duplica — o da outra placa de vídeo num notebook híbrido, por exemplo — é erro, e quem
    /// chama volta ao Graphics Capture. `on_frame` devolve `Break` para parar.
    pub fn start<F>(monitor: Monitor, frame_rate: u32, show_cursor: bool, on_frame: F) -> Result<Self, String>
    where
        F: FnMut(&DuplicatedFrame<'_>) -> ControlFlow<()> + Send + 'static,
    {
        let stop = Arc::new(AtomicBool::new(false));
        let (opened, answer) = sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("unkvoid-duplicacao".into())
            .spawn({
                let stop = Arc::clone(&stop);

                move || run(monitor, frame_rate, show_cursor, on_frame, &stop, &opened)
            })
            .map_err(|failure| failure.to_string())?;

        match answer.recv_timeout(OPENING) {
            Ok(Ok(())) => Ok(Self { stop, thread: Some(thread) }),
            Ok(Err(failure)) => Err(failure),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);

                Err("o Desktop Duplication não respondeu".into())
            }
        }
    }

    /// Parou sozinha: quem recebia os quadros pediu, ou a thread caiu.
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|thread| !thread.is_finished())
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Duplication {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Um quadro só, em JPEG, para a prévia do seletor de tela: pelo Graphics Capture a borda
/// piscava no monitor cada vez que o seletor abria.
pub fn preview(monitor: Monitor, path: &Path) -> Result<(), String> {
    let mut duplication = open(monitor)?;

    // O primeiro quadro depois de abrir já traz a tela inteira; os seguintes, só o que mudou.
    for _ in 0..5 {
        match duplication.acquire_next_frame(WAIT_MS * 2) {
            Ok(mut frame) => return frame.save_as_image(path, ImageFormat::Jpeg).map_err(|failure| failure.to_string()),
            Err(DuplicationError::Timeout) => {}
            Err(failure) => return Err(failure.to_string()),
        }
    }

    Err("a tela não mandou nenhum quadro".into())
}

/// Em BGRA de 8 bits, o mesmo do Graphics Capture: num monitor HDR a duplicação daria meio
/// float por canal, que o encoder não lê.
fn open(monitor: Monitor) -> Result<DxgiDuplicationApi, String> {
    DxgiDuplicationApi::new_options(monitor, &[DxgiDuplicationFormat::Bgra8]).map_err(|failure| failure.to_string())
}

/// O que a volta do laço deu.
enum Turn {
    Waited,
    /// A duplicação esperou o prazo e a tela não mudou.
    Idle,
    Lost(String),
    Stopped,
}

/// O laço da duplicação. Todo quadro é copiado para o `canvas`, mesmo o que chega antes da vez
/// (`due`): a duplicação só entrega o que mudou, e o último quadro de uma rajada largado não
/// volta. Quando a tela para com uma imagem esperando a vez, ela sai na vez dela — num segundo
/// `match`, porque o quadro emprestado da duplicação só é devolvido no fim do primeiro.
fn run<F>(monitor: Monitor, frame_rate: u32, show_cursor: bool, mut on_frame: F, stop: &AtomicBool, opened: &SyncSender<Result<(), String>>)
where
    F: FnMut(&DuplicatedFrame<'_>) -> ControlFlow<()>,
{
    let mut duplication = match open(monitor) {
        Ok(duplication) => {
            let _ = opened.send(Ok(()));

            duplication
        }
        Err(failure) => {
            let _ = opened.send(Err(failure));

            return;
        }
    };

    tracing::info!("captura: monitor pelo Desktop Duplication, sem a borda amarela");

    let mut origin = desktop_origin(&duplication);
    let interval = Duration::from_secs_f64(1.0 / f64::from(frame_rate.max(1)));
    let mut next_due = Instant::now();
    let mut canvas: Option<Canvas> = None;
    let mut cursor = Cursor::default();
    let mut unsent = false;

    while !stop.load(Ordering::Relaxed) {
        let wait = if unsent { next_due.saturating_duration_since(Instant::now()).as_millis().clamp(1, u128::from(WAIT_MS)) as u32 } else { WAIT_MS };
        let turn = match duplication.acquire_next_frame(wait) {
            Ok(frame) => {
                let (width, height) = (frame.width(), frame.height());

                if !canvas.as_ref().is_some_and(|canvas| canvas.fits(width, height)) {
                    canvas = Canvas::new(frame.device(), width, height)
                        .inspect_err(|failure| tracing::warn!(%failure, "captura: a cópia do quadro não foi criada"))
                        .ok();
                }

                match canvas.as_ref() {
                    Some(painted) => {
                        // A cópia sai mesmo do quadro que não é a vez: a duplicação só entrega o que
                        // mudou, e o último quadro de uma rajada (a rolagem que parou, a última
                        // tecla) jogado fora não volta — quem assistia ficava com a imagem do meio
                        // da rajada até a tela mudar de novo.
                        // SAFETY: as duas texturas são do mesmo device e do mesmo tamanho, e a
                        // da duplicação vale até o próximo `acquire_next_frame`.
                        unsafe { frame.device_context().CopyResource(&painted.texture, frame.texture()) };

                        if due(Instant::now(), &mut next_due, interval) {
                            unsent = false;

                            if show_cursor {
                                cursor.draw(&painted.surface, origin);
                            }

                            let delivered = on_frame(&DuplicatedFrame {
                                texture: &painted.texture,
                                device: frame.device(),
                                context: frame.device_context(),
                                width,
                                height,
                                timestamp_ns: present_ns(frame.frame_info().LastPresentTime),
                            });

                            if delivered.is_break() { Turn::Stopped } else { Turn::Waited }
                        } else {
                            unsent = true;

                            Turn::Waited
                        }
                    }
                    None => Turn::Lost("a cópia do quadro não foi criada".into()),
                }
            }
            Err(DuplicationError::Timeout) => Turn::Idle,
            Err(failure) => Turn::Lost(failure.to_string()),
        };

        let turn = match (turn, canvas.as_ref()) {
            (Turn::Idle, Some(painted)) if unsent && due(Instant::now(), &mut next_due, interval) => {
                unsent = false;

                if show_cursor {
                    cursor.draw(&painted.surface, origin);
                }

                let delivered = on_frame(&DuplicatedFrame {
                    texture: &painted.texture,
                    device: duplication.device(),
                    context: duplication.device_context(),
                    width: painted.width,
                    height: painted.height,
                    timestamp_ns: present_ns(0),
                });

                if delivered.is_break() { Turn::Stopped } else { Turn::Waited }
            }
            (turn, _) => turn,
        };

        match turn {
            Turn::Waited | Turn::Idle => {}
            Turn::Stopped => return,
            Turn::Lost(failure) => {
                tracing::info!(%failure, "captura: a duplicação caiu, retomando");
                canvas = None;
                unsent = false;

                match reopen(monitor, stop) {
                    // A troca de resolução que derrubou a duplicação pode ter mudado onde o
                    // monitor fica na área de trabalho, e o cursor seria desenhado fora do lugar.
                    Some(fresh) => {
                        duplication = fresh;
                        origin = desktop_origin(&duplication);
                    }
                    None => return,
                }
            }
        }
    }
}

/// Se é a vez de um quadro sair, e marca a próxima. O monitor manda na frequência dele (240 Hz,
/// num monitor de jogo), e o teto é o do encoder: contar de `next_due`, e não de agora, mantém a
/// média no fps pedido. Um quarto de quadro de folga, como o `FramePacer` do `media`: a 60 Hz o
/// quadro não chega a cada 16 666 µs exatos, e sem folga o que chegava um tico adiantado ficava
/// de fora — a transmissão caía para uns 40 fps aos trancos.
fn due(now: Instant, next_due: &mut Instant, interval: Duration) -> bool {
    if now + interval / 4 < *next_due {
        return false;
    }

    *next_due = if now.saturating_duration_since(*next_due) > interval { now + interval } else { *next_due + interval };

    true
}

/// Insiste até a área de trabalho voltar, ou até pedirem para parar.
fn reopen(monitor: Monitor, stop: &AtomicBool) -> Option<DxgiDuplicationApi> {
    while !stop.load(Ordering::Relaxed) {
        if let Ok(duplication) = open(monitor) {
            return Some(duplication);
        }

        std::thread::sleep(RETRY);
    }

    None
}

/// O canto do monitor nas coordenadas da área de trabalho: o cursor vem nelas.
fn desktop_origin(duplication: &DxgiDuplicationApi) -> POINT {
    // SAFETY: só lê a descrição da saída, que a duplicação mantém viva.
    unsafe { duplication.output().GetDesc1() }
        .map(|description| POINT { x: description.DesktopCoordinates.left, y: description.DesktopCoordinates.top })
        .unwrap_or_default()
}

/// O `LastPresentTime` é do QPC, em tiques; zero quando só o cursor mexeu, e aí vale agora.
fn present_ns(last_present: i64) -> u64 {
    let mut frequency = 0_i64;
    let mut ticks = last_present;

    // SAFETY: as duas leituras só escrevem nos inteiros passados.
    unsafe {
        let _ = QueryPerformanceFrequency(&mut frequency);

        if ticks <= 0 {
            let _ = QueryPerformanceCounter(&mut ticks);
        }
    }

    (ticks.max(0) as u128 * 1_000_000_000 / frequency.max(1) as u128) as u64
}

/// A cópia do quadro onde o cursor é desenhado. Compatível com o GDI para o `DrawIconEx`, e
/// alvo de desenho, que é o que o GDI exige dela.
struct Canvas {
    texture: ID3D11Texture2D,
    surface: IDXGISurface1,
    width: u32,
    height: u32,
}

impl Canvas {
    fn new(device: &ID3D11Device, width: u32, height: u32) -> ::windows::core::Result<Self> {
        let descriptor = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: D3D11_RESOURCE_MISC_GDI_COMPATIBLE.0 as u32,
        };
        let mut texture: Option<ID3D11Texture2D> = None;

        // SAFETY: a descrição vive até o fim da chamada, e a textura sai no `Option`.
        unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut texture))? };

        let texture = texture.ok_or_else(::windows::core::Error::empty)?;
        let surface = texture.cast()?;

        Ok(Self { texture, surface, width, height })
    }

    fn fits(&self, width: u32, height: u32) -> bool {
        self.width == width && self.height == height
    }
}

/// O cursor do Windows, com o ponto de clique do desenho atual guardado: perguntar ao
/// Windows a cada quadro criaria e apagaria dois bitmaps por quadro.
#[derive(Default)]
struct Cursor {
    shape: Option<(isize, POINT)>,
}

impl Cursor {
    fn draw(&mut self, surface: &IDXGISurface1, origin: POINT) {
        let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };

        // SAFETY: a estrutura vai com o tamanho dela preenchido.
        if unsafe { GetCursorInfo(&mut info) }.is_err() || info.flags.0 & CURSOR_SHOWING.0 == 0 || info.hCursor.is_invalid() {
            return;
        }

        let hotspot = self.hotspot(info.hCursor);

        paint(surface, info.hCursor, info.ptScreenPos.x - origin.x - hotspot.x, info.ptScreenPos.y - origin.y - hotspot.y);
    }

    fn hotspot(&mut self, cursor: HCURSOR) -> POINT {
        let key = cursor.0 as isize;

        if let Some((known, hotspot)) = self.shape
            && known == key
        {
            return hotspot;
        }

        let mut info = ICONINFO::default();

        // SAFETY: o `GetIconInfo` cria os dois bitmaps do desenho, e eles saem logo aqui.
        let hotspot = unsafe {
            if GetIconInfo(HICON(cursor.0), &mut info).is_err() {
                return POINT::default();
            }

            let _ = DeleteObject(info.hbmMask.into());
            let _ = DeleteObject(info.hbmColor.into());

            POINT { x: info.xHotspot as i32, y: info.yHotspot as i32 }
        };

        self.shape = Some((key, hotspot));

        hotspot
    }
}

/// O cursor na cópia do quadro, pelo GDI do próprio Windows. Fora do monitor, o GDI corta.
fn paint(surface: &IDXGISurface1, cursor: HCURSOR, x: i32, y: i32) {
    // SAFETY: o contexto do GDI da superfície vale até o `ReleaseDC`, que vem logo depois, e o
    // `DrawIconEx` só desenha nele.
    unsafe {
        let Ok(context) = surface.GetDC(false) else {
            return;
        };

        let _ = DrawIconEx(context, x, y, HICON(cursor.0), 0, 0, 0, None, DI_NORMAL);
        let _ = surface.ReleaseDC(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ::windows::Win32::Graphics::Direct3D11::{D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING};
    use ::windows::Win32::UI::WindowsAndMessaging::{IDC_ARROW, LoadCursorW};

    /// A seta do Windows desenhada na cópia: os pixels debaixo dela mudam, e os longe dela não.
    /// É o caminho do cursor sem depender da tela de quem roda — um jogo esconde o cursor.
    #[test]
    fn the_cursor_is_painted_on_the_copy_of_the_frame() {
        let (device, context) = windows_capture::d3d11::create_d3d_device().expect("device");
        let canvas = Canvas::new(&device, 64, 64).expect("cópia");
        let before = download(&device, &context, &canvas.texture, 64, 64);
        // SAFETY: a seta padrão é do sistema, sem módulo nenhum.
        let arrow = unsafe { LoadCursorW(None, IDC_ARROW) }.expect("seta");

        paint(&canvas.surface, arrow, 8, 8);

        let after = download(&device, &context, &canvas.texture, 64, 64);
        let changed = |from_x: usize, to_x: usize, from_y: usize, to_y: usize| {
            (from_y..to_y).flat_map(|y| (from_x..to_x).map(move |x| (y * 64 + x) * 4)).filter(|&at| before[at..at + 3] != after[at..at + 3]).count()
        };

        assert!(changed(8, 24, 8, 30) > 20, "a seta não apareceu");
        assert_eq!(changed(40, 64, 40, 64), 0, "o resto do quadro mudou");
    }

    /// Um monitor de 60 Hz com o encoder em 60 fps: o quadro que chega um tico adiantado passa.
    /// Sem a folga, mais de um terço caía, aos trancos.
    #[test]
    fn a_jittery_60_hz_monitor_keeps_60_fps() {
        let interval = Duration::from_secs_f64(1.0 / 60.0);
        let start = Instant::now();
        let mut next_due = start;
        let admitted = (0..600_u32)
            .filter(|frame| {
                let jitter = if frame % 2 == 0 { Duration::from_micros(400) } else { Duration::ZERO };

                due(start + interval * *frame + Duration::from_millis(1) - jitter, &mut next_due, interval)
            })
            .count();

        assert_eq!(admitted, 600);
    }

    /// Um monitor de 144 Hz com o encoder em 60 fps: a média fica em 60, sem rajada.
    #[test]
    fn a_144_hz_monitor_is_held_to_60_fps() {
        let interval = Duration::from_secs_f64(1.0 / 60.0);
        let start = Instant::now();
        let mut next_due = start;
        let admitted = (0..1_440_u32).filter(|frame| due(start + Duration::from_secs_f64(f64::from(*frame) / 144.0), &mut next_due, interval)).count();

        assert!((590..=610).contains(&admitted), "{admitted} quadros em 10 s");
    }

    /// Na máquina de verdade: o monitor principal duplicado por um segundo, e o primeiro quadro,
    /// já com o cursor, salvo em BMP na pasta temporária para olhar. Tirado da GPU só aqui.
    #[test]
    #[ignore = "precisa de um monitor de verdade"]
    fn the_primary_monitor_is_duplicated_with_the_cursor() {
        let saved = Arc::new(std::sync::Mutex::new(None::<Vec<u8>>));
        let frames = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let size = Arc::new(AtomicU32Pair::default());
        let mut duplication = Duplication::start(Monitor::primary().expect("monitor"), 60, true, {
            let (saved, frames, size) = (Arc::clone(&saved), Arc::clone(&frames), Arc::clone(&size));

            move |frame| {
                frames.fetch_add(1, Ordering::Relaxed);

                let mut saved = saved.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

                if saved.is_none() {
                    size.store(frame.width, frame.height);
                    *saved = Some(download(frame.device, frame.context, frame.texture, frame.width, frame.height));
                }

                ControlFlow::Continue(())
            }
        })
        .expect("a duplicação não abriu");

        std::thread::sleep(Duration::from_secs(1));
        duplication.stop();

        let pixels = saved.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take().expect("nenhum quadro chegou");
        let (frame_width, frame_height) = size.load();
        let path = std::env::temp_dir().join("unkvoid-duplicacao.bmp");

        std::fs::write(&path, bmp(&pixels, frame_width, frame_height)).expect("bmp");
        println!("{} quadros; o primeiro em {}", frames.load(Ordering::Relaxed), path.display());
    }

    #[derive(Default)]
    struct AtomicU32Pair(std::sync::atomic::AtomicU64);

    impl AtomicU32Pair {
        fn store(&self, first: u32, second: u32) {
            self.0.store((u64::from(first) << 32) | u64::from(second), Ordering::Relaxed);
        }

        fn load(&self) -> (u32, u32) {
            let packed = self.0.load(Ordering::Relaxed);

            ((packed >> 32) as u32, packed as u32)
        }
    }

    fn download(device: &ID3D11Device, context: &ID3D11DeviceContext, texture: &ID3D11Texture2D, width: u32, height: u32) -> Vec<u8> {
        let descriptor = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);

        unsafe {
            device.CreateTexture2D(&descriptor, None, Some(&mut staging)).expect("staging");

            let staging = staging.expect("staging");
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();

            context.CopyResource(&staging, texture);
            context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).expect("map");

            for row in 0..height as usize {
                let start = mapped.pData.cast::<u8>().add(row * mapped.RowPitch as usize);

                pixels.extend_from_slice(std::slice::from_raw_parts(start, width as usize * 4));
            }

            context.Unmap(&staging, 0);
        }

        pixels
    }

    fn bmp(pixels: &[u8], width: u32, height: u32) -> Vec<u8> {
        let row = width as usize * 4;
        let mut file = Vec::with_capacity(54 + pixels.len());

        file.extend_from_slice(b"BM");
        file.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&54_u32.to_le_bytes());
        file.extend_from_slice(&40_u32.to_le_bytes());
        file.extend_from_slice(&width.to_le_bytes());
        file.extend_from_slice(&height.to_le_bytes());
        file.extend_from_slice(&1_u16.to_le_bytes());
        file.extend_from_slice(&32_u16.to_le_bytes());
        file.extend_from_slice(&[0; 24]);

        for line in (0..height as usize).rev() {
            file.extend_from_slice(&pixels[line * row..(line + 1) * row]);
        }

        file
    }
}
