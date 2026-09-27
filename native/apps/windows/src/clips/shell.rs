//! A cola com o Windows que o app precisa e o Slint não tem: pastas, registro, miniaturas do
//! Explorer, Lixeira, seletor de pasta e foco de janela.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use anyhow::bail;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, CreateEventW, CreateMutexW, GetCurrentProcessId, GetCurrentThread,
    GetCurrentThreadId, SetEvent, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOLDERID_Videos, FO_DELETE,
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, IShellItem,
    IShellItemImageFactory, KF_FLAG_DEFAULT, SHCreateItemFromParsingName, SHFILEOPSTRUCTW,
    SHFileOperationW, SHGetKnownFolderPath, SIGDN_FILESYSPATH, SIIGBF_RESIZETOFIT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowTextW,
    GetWindowThreadProcessId, SetForegroundWindow,
};
use windows::core::{HSTRING, PCWSTR, w};

const REGISTRY_KEY: PCWSTR = w!("Software\\UnkvoidClips");
const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("UnkvoidClips");

/// `%APPDATA%\com.unkvoid.desktop`: o que segue a pessoa, a mesma pasta do `storage`.
pub fn roaming_folder() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("com.unkvoid.desktop")
}

/// `%LOCALAPPDATA%\com.unkvoid.desktop`: o que é desta máquina (o buffer do replay, o log).
pub fn local_folder() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("com.unkvoid.desktop")
}

/// As escolhas de quem usava o UnkvoidClips, o app separado de antes.
pub fn legacy_settings() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("UnkvoidClips").join("settings.json")
}

/// A pasta de vídeos de verdade, que o OneDrive ou a própria pessoa pode ter levado para
/// outro disco.
pub fn videos_folder() -> PathBuf {
    unsafe {
        match SHGetKnownFolderPath(&FOLDERID_Videos, KF_FLAG_DEFAULT, None) {
            Ok(path) => {
                let folder = PathBuf::from(path.to_string().unwrap_or_default());

                CoTaskMemFree(Some(path.0.cast_const().cast()));

                folder
            }
            Err(_) => PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Videos"),
        }
    }
}

/// A pasta que a pessoa escolheu no instalador, se ele gravou uma.
pub fn installer_clips_folder() -> Option<PathBuf> {
    let mut buffer = [0_u16; 1024];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            REGISTRY_KEY,
            w!("ClipsFolder"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };

    if result.is_err() {
        return None;
    }

    let text = String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]);

    (!text.trim().is_empty()).then(|| PathBuf::from(text))
}

/// Liga ou desliga o início com o Windows. Regravado a cada abertura: uma atualização que mude
/// o caminho do executável não deixa o início apontando para nada.
///
/// Por tarefa agendada e não pela chave `Run`: o app roda como administrador, e o Windows não
/// abre programa elevado pela `Run` sem pedir UAC a cada logon. A tarefa "ao fazer logon, com
/// privilégios máximos" abre sem perguntar.
pub fn set_autostart(enabled: bool) {
    // A 0.1.0 usava a chave `Run`; ela abriria uma segunda cópia, pedindo UAC, a cada logon.
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE);
    }

    let result = if enabled { register_logon_task() } else { run_schtasks(&["/Delete", "/F", "/TN", TASK_NAME]) };

    if let Err(error) = result {
        tracing::warn!(error = %format!("{error:#}"), enabled, "início com o Windows: a tarefa agendada não foi gravada");
    }
}

const TASK_NAME: &str = "Unkvoid";

fn register_logon_task() -> anyhow::Result<()> {
    let executable = std::env::current_exe()?;
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let escape = |text: &str| text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");

    // Prioridade 4 é a normal: a padrão da tarefa agendada (7) abre o processo abaixo do
    // normal, e a captura perderia quadros para qualquer coisa rodando junto. Sem limite de
    // tempo (PT0S): o padrão mata a tarefa depois de três dias ligada.
    let definition = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Unkvoid: aberto na bandeja ao entrar no Windows, com o replay dos Clips.</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <IdleSettings><StopOnIdleEnd>false</StopOnIdleEnd><RestartOnIdle>false</RestartOnIdle></IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{}</Command><Arguments>--background</Arguments></Exec></Actions>
</Task>
"#,
        escape(&executable.display().to_string()),
        user = escape(&user),
    );
    let file = std::env::temp_dir().join("Unkvoid-task.xml");
    // O schtasks lê o XML em UTF-16 com BOM, como o próprio Agendador exporta.
    let bytes: Vec<u8> = std::iter::once(0xFEFF_u16).chain(definition.encode_utf16()).flat_map(u16::to_le_bytes).collect();

    std::fs::write(&file, bytes)?;

    let result = run_schtasks(&["/Create", "/F", "/TN", TASK_NAME, "/XML", &file.to_string_lossy()]);
    let _ = std::fs::remove_file(&file);

    result
}

fn run_schtasks(arguments: &[&str]) -> anyhow::Result<()> {
    // Sem isto o schtasks abre uma janela de console a cada abertura do app.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let output = std::process::Command::new("schtasks").args(arguments).creation_flags(CREATE_NO_WINDOW).output()?;

    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }

    Ok(())
}

/// Garante uma instância só. Devolve o evento que a segunda instância sinaliza para esta
/// mostrar a janela, ou `None` quando esta é a segunda — que já avisou a primeira.
pub fn single_instance() -> anyhow::Result<Option<HANDLE>> {
    unsafe {
        // Fica aberto até o processo morrer (o `HANDLE` não fecha sozinho): é ele que diz
        // "já tem um rodando".
        let _instance = CreateMutexW(None, true, w!("Local\\Unkvoid"))?;
        let already_running = GetLastError() == ERROR_ALREADY_EXISTS;
        let event = CreateEventW(None, false, false, w!("Local\\Unkvoid-show"))?;

        if already_running {
            let _ = SetEvent(event);

            return Ok(None);
        }

        Ok(Some(event))
    }
}

/// Baixa a prioridade de CPU e de disco da thread que chama. É o que deixa salvar trinta
/// minutos de replay no meio da partida sem o jogo engasgar esperando o mesmo SSD.
pub fn lower_thread_priority() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

/// A miniatura do vídeo como o Explorer a mostra, em RGBA. Precisa do COM na thread.
pub fn thumbnail(path: &Path, width: i32, height: i32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        let bitmap = factory.GetImage(SIZE { cx: width, cy: height }, SIIGBF_RESIZETOFIT).ok()?;
        let object = HGDIOBJ(bitmap.0);
        let mut description = BITMAP::default();

        GetObjectW(object, std::mem::size_of::<BITMAP>() as i32, Some(std::ptr::from_mut(&mut description).cast()));

        let (bitmap_width, bitmap_height) = (description.bmWidth, description.bmHeight);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bitmap_width,
                // Negativo: linhas de cima para baixo, como o Slint lê.
                biHeight: -bitmap_height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0_u8; (bitmap_width * bitmap_height * 4) as usize];
        let screen = GetDC(None);
        let lines = GetDIBits(screen, bitmap, 0, bitmap_height as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);

        ReleaseDC(None, screen);
        let _ = DeleteObject(object);

        if lines == 0 {
            return None;
        }

        // O Windows entrega BGRA com alfa zerado em imagem sem transparência.
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }

        Some((bitmap_width as u32, bitmap_height as u32, pixels))
    }
}

/// Manda para a Lixeira em vez de apagar: um clique errado na galeria não perde a jogada.
pub fn move_to_recycle_bin(path: &Path) -> anyhow::Result<()> {
    let from: Vec<u16> = path.as_os_str().encode_wide_with_terminator();
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let result = unsafe { SHFileOperationW(&mut operation) };

    if result != 0 || operation.fAnyOperationsAborted.as_bool() {
        bail!("o Windows não mandou {} para a Lixeira (código {result})", path.display());
    }

    Ok(())
}

pub fn open_folder(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

/// Abre o Explorer com o arquivo selecionado. O `/select,` tem de ir colado no caminho entre
/// aspas, que é o que o Explorer entende; o `arg` do Rust poria aspas em volta dos dois.
pub fn show_in_folder(path: &Path) {
    let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
}

/// O seletor de pasta do Windows, começando em `start`.
pub fn pick_folder(owner: HWND, start: &Path) -> Option<PathBuf> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;

        dialog.SetOptions(dialog.GetOptions().ok()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;

        if let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(start.as_os_str()), None) {
            let _ = dialog.SetFolder(&folder);
        }

        dialog.Show(Some(owner)).ok()?;

        let chosen = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = PathBuf::from(chosen.to_string().ok()?);

        CoTaskMemFree(Some(chosen.0.cast_const().cast()));

        Some(path)
    }
}

pub fn foreground_window() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// O título da janela em primeiro plano, que vira o nome do clipe — o do jogo, quase sempre.
/// Vazio quando a janela é deste app.
pub fn window_title(window: HWND) -> String {
    unsafe {
        let mut process = 0_u32;

        GetWindowThreadProcessId(window, Some(&mut process));

        if process == GetCurrentProcessId() {
            return String::new();
        }

        let mut buffer = [0_u16; 256];
        let length = GetWindowTextW(window, &mut buffer);

        String::from_utf16_lossy(&buffer[..length.max(0) as usize])
    }
}

/// O nome do executável dono da janela ("UAGame.exe"). Vazio quando o Windows não diz, ou
/// quando a janela é deste app.
pub fn process_name(window: HWND) -> String {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows::core::PWSTR;

    unsafe {
        let mut process = 0_u32;

        GetWindowThreadProcessId(window, Some(&mut process));

        if process == 0 || process == GetCurrentProcessId() {
            return String::new();
        }

        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) else {
            return String::new();
        };
        let mut buffer = [0_u16; 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut length);

        let _ = CloseHandle(handle);

        if queried.is_err() {
            return String::new();
        }

        Path::new(&String::from_utf16_lossy(&buffer[..length as usize]))
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Se a janela cobre o monitor dela inteiro sem barra de título: tela cheia ou janela sem
/// borda, o jeito como os jogos rodam. Janela maximizada tem barra de título, e com a barra de
/// tarefas oculta também cobre o monitor (a área de trabalho é do explorer.exe, que a
/// `gallery::target` já manda para `Desktop`).
pub fn is_fullscreen(window: HWND) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    use windows::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetWindowLongW, GetWindowRect, WS_CAPTION};

    unsafe {
        let mut bounds = RECT::default();
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };

        if GetWindowLongW(window, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0
            || GetWindowRect(window, &mut bounds).is_err()
            || !GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &mut info).as_bool()
        {
            return false;
        }

        let monitor = info.rcMonitor;

        bounds.left <= monitor.left && bounds.top <= monitor.top && bounds.right >= monitor.right && bounds.bottom >= monitor.bottom
    }
}

/// Traz a janela para frente mesmo com outro app no foco. O Windows só deixa quem recebeu o
/// último clique ou tecla trocar o foco; atar a fila de entrada à da janela em primeiro plano
/// por um instante é o jeito de se passar por ela.
pub fn bring_to_front(window: HWND) {
    unsafe {
        let foreground = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground, None);
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != current_thread && AttachThreadInput(current_thread, foreground_thread, true).as_bool();

        let _ = BringWindowToTop(window);
        let _ = SetForegroundWindow(window);

        if attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }
    }
}

/// O monitor principal, em pixels físicos. O principal é sempre o que contém o ponto (0, 0).
pub fn primary_monitor_rect() -> (i32, i32, i32, i32) {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint};

    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let area = info.rcMonitor;

        (area.left, area.top, area.right - area.left, area.bottom - area.top)
    }
}

/// Tira a janela da barra de tarefas e do Alt+Tab: o painel do Alt+Z não é um programa a
/// mais aberto, é uma camada por cima do jogo.
/// Tira a janela já aberta da barra de tarefas e do Alt+Tab. O winit reescreve o estilo a cada
/// `show()`, depois de a barra já ter criado o botão, então o estilo sozinho não o remove: quem
/// remove é a própria barra de tarefas.
pub fn hide_from_taskbar(window: HWND) {
    use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW};

    unsafe {
        let style = GetWindowLongPtrW(window, GWL_EXSTYLE);

        SetWindowLongPtrW(window, GWL_EXSTYLE, (style | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize));

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        if let Ok(taskbar) = CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
            && taskbar.HrInit().is_ok()
        {
            let _ = taskbar.DeleteTab(window);
        }
    }
}

/// Se há um jogo em tela cheia exclusiva agora. Nesse modo o jogo fala direto com a placa, e
/// qualquer janela por cima (o painel do Alt+Z) o tira da tela cheia e o minimiza. O
/// GeForce Experience escapava disso desenhando dentro do jogo, com código injetado; aqui,
/// sem injeção, o jeito é não abrir janela nenhuma.
pub fn exclusive_fullscreen() -> bool {
    use windows::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};

    unsafe { SHQueryUserNotificationState() }.is_ok_and(|state| state == QUNS_RUNNING_D3D_FULL_SCREEN)
}

/// O som de "feito" (ou de erro) do Windows: em tela cheia exclusiva o aviso na tela não
/// aparece, e o som é a única confirmação de que o replay foi salvo.
pub fn play_sound(success: bool) {
    use windows::Win32::System::Diagnostics::Debug::MessageBeep;
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONHAND, MB_OK};

    unsafe {
        let _ = MessageBeep(if success { MB_OK } else { MB_ICONHAND });
    }
}

/// Devolve a janela ao tamanho de antes se ela foi minimizada. Rede de segurança do painel:
/// jogo que se minimiza sozinho ao perder o foco volta para a tela quando o painel fecha.
pub fn restore_if_minimized(window: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SW_RESTORE, ShowWindow};

    unsafe {
        if IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
    }
}

/// Uma miniatura do que o monitor mostra agora, em RGBA, para escolher qual gravar. Pelo GDI:
/// uma cópia reduzida da tela, sem abrir sessão de captura (que acenderia a borda amarela no
/// Windows 10).
pub fn monitor_preview(monitor: isize, width: i32, height: i32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, GetMonitorInfoW, HALFTONE, HMONITOR, MONITORINFO,
        SRCCOPY, SelectObject, SetStretchBltMode, StretchBlt,
    };

    unsafe {
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };

        if !GetMonitorInfoW(HMONITOR(monitor as *mut _), &mut info).as_bool() {
            return None;
        }

        let area = info.rcMonitor;
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, bitmap.into());

        // HALFTONE faz a média dos pixels ao reduzir: sem ele o texto da tela vira ruído.
        SetStretchBltMode(memory, HALFTONE);

        let copied = StretchBlt(
            memory,
            0,
            0,
            width,
            height,
            Some(screen),
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
            SRCCOPY,
        )
        .as_bool();

        SelectObject(memory, previous);

        let mut information = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0_u8; (width * height * 4) as usize];
        let lines = GetDIBits(memory, bitmap, 0, height as u32, Some(pixels.as_mut_ptr().cast()), &mut information, DIB_RGB_COLORS);

        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);

        if !copied || lines == 0 {
            return None;
        }

        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }

        Some((width as u32, height as u32, pixels))
    }
}

/// Um retângulo arredondado em pixels físicos: x, y, largura, altura e raio.
pub type RoundedRect = (i32, i32, i32, i32, i32);

/// Recorta a janela nos retângulos arredondados `(x, y, largura, altura, raio)`, em pixels
/// físicos da própria janela: fora deles ela não existe, nem para desenho nem para clique. É o
/// que deixa o painel do Alt+Z ser só a barra e o quadro, sem uma caixa em volta.
pub fn shape_window(window: HWND, shapes: &[RoundedRect]) {
    use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, CreateRoundRectRgn, RGN_OR, SetWindowRgn};

    unsafe {
        let region = CreateRectRgn(0, 0, 0, 0);

        for &(x, y, width, height, radius) in shapes {
            let piece = CreateRoundRectRgn(x, y, x + width + 1, y + height + 1, radius * 2, radius * 2);

            CombineRgn(Some(region), Some(region), Some(piece), RGN_OR);
            let _ = DeleteObject(piece.into());
        }

        // A região passa a ser do Windows depois desta chamada: não se apaga.
        SetWindowRgn(window, Some(region), true);
    }
}
/// Onde a janela está e o que dela aparece, para o log: o retângulo, se está visível e a caixa
/// do recorte.
pub fn describe_window(window: HWND) -> String {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::GetWindowRgnBox;
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};

    unsafe {
        let mut bounds = RECT::default();
        let mut region = RECT::default();
        let _ = GetWindowRect(window, &mut bounds);
        let kind = GetWindowRgnBox(window, &mut region);

        format!(
            "{},{} {}x{} visible={} region={:?} {},{} {}x{}",
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            IsWindowVisible(window).as_bool(),
            kind,
            region.left,
            region.top,
            region.right - region.left,
            region.bottom - region.top,
        )
    }
}

/// Sem a moldura do Windows. A janela sem borda do winit ainda tem o estilo de janela com
/// moldura, e com um recorte próprio (`SetWindowRgn`) o Windows volta a desenhar a moldura
/// clássica quando ela perde o foco: duas faixas brancas nas laterais do painel.
pub fn suppress_frame(window: HWND) {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{WM_NCACTIVATE, WM_NCPAINT};

    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, _: usize, _: usize) -> LRESULT {
        match message {
            // -1 é o "troque o estado sem redesenhar a moldura" do `DefWindowProc`.
            WM_NCACTIVATE => unsafe { DefSubclassProc(window, message, wparam, LPARAM(-1)) },
            WM_NCPAINT => LRESULT(0),
            _ => unsafe { DefSubclassProc(window, message, wparam, lparam) },
        }
    }

    // Mesma janela e mesmo id: chamar de novo a cada abertura não empilha nada.
    unsafe {
        let _ = SetWindowSubclass(window, Some(procedure), 1, 0);
    }
}

/// O HWND de uma janela do Slint.
pub fn window_handle(window: &slint::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    match window.window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
        _ => None,
    }
}

trait WideWithTerminator {
    /// Caminho em UTF-16 com dois nulos no fim: o `SHFileOperationW` aceita uma lista de
    /// caminhos, e é o nulo duplo que diz onde ela acaba.
    fn encode_wide_with_terminator(&self) -> Vec<u16>;
}

impl WideWithTerminator for std::ffi::OsStr {
    fn encode_wide_with_terminator(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;

        self.encode_wide().chain([0, 0]).collect()
    }
}
