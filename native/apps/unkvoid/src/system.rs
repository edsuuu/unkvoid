//! O que o app pede ao sistema e não é tela: o endereço do site, o nome desta máquina, o
//! navegador, o instalador da versão nova e o envio do log com erro.

use core_app::api::Api;

use crate::backend::{Courier, Event};

const DEFAULT_SERVER: &str = "https://unkvoid.com";

#[cfg(target_os = "windows")]
pub fn foreground_activity_name() -> Option<String> {
    let candidate = crate::shell::foreground_activity_candidate()?;
    core_app::activity::choose_activity(&[candidate])
        .map(|name| name.chars().take(80).collect())
        .filter(|name: &String| !name.trim().is_empty())
}

#[cfg(not(target_os = "windows"))]
pub fn foreground_activity_name() -> Option<String> {
    None
}

/// O site que o app usa: o de produção, ou o de `UNKVOID_SERVER` para a pilha local.
pub fn server() -> String {
    std::env::var("UNKVOID_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.to_owned())
}

/// O sistema e o nome da máquina, como aparecem na lista de sessões da conta.
pub fn device_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|host| host.trim().to_owned())
        .filter(|host| !host.is_empty());

    match host {
        Some(host) => format!("{}-{host}", std::env::consts::OS),
        None => std::env::consts::OS.to_owned(),
    }
}

/// A pasta do log do dia: a local do app no Windows, a de estado do XDG no resto.
pub fn log_folder() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    return Some(crate::shell::local_folder());

    #[cfg(not(target_os = "windows"))]
    std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|state| state.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".local/state"))
        })
        .map(|state| {
            state.join(if crate::BETA {
                "unkvoid-beta"
            } else {
                "unkvoid"
            })
        })
}

/// O que deu erro no log vai ao site de meio em meio minuto, enquanto o app estiver aberto: o
/// problema de quem usa chega a quem conserta sem ninguém pedir arquivo. O pedaço só sai do
/// pendente quando o site confirma.
pub async fn report_errors(api: &Api) {
    let Some(folder) = log_folder() else {
        return;
    };

    #[cfg(target_os = "windows")]
    loop {
        if let Some(pending) = crate::logbook::unreported(&folder)
            && api
                .report_error(
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::OS,
                    &pending.log,
                )
                .await
        {
            pending.sent();
        }

        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }

    #[cfg(not(target_os = "windows"))]
    core_app::logbook::report_errors(api, &folder).await;
}

/// Envia o trecho pendente quando a pessoa pede; o marcador só avança após a confirmação do site.
pub async fn report_errors_once(api: &Api) -> bool {
    let Some(folder) = log_folder() else {
        return false;
    };

    #[cfg(target_os = "windows")]
    let pending = crate::logbook::unreported(&folder);
    #[cfg(not(target_os = "windows"))]
    let pending = core_app::logbook::unreported(&folder);

    let Some(pending) = pending else {
        return true;
    };

    if api
        .report_error(
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            &pending.log,
        )
        .await
    {
        pending.sent();
        true
    } else {
        false
    }
}

/// O seletor de arquivo do sistema, só com as imagens que o site aceita (jpeg, png, webp).
/// Prende a thread até a escolha.
pub fn pick_image() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    return crate::shell::pick_image();

    // ponytail: no Linux o seletor é o `zenity`, que quase todo desktop tem; sem ele nada abre. A
    // saída é o portal do freedesktop (`org.freedesktop.portal.FileChooser`).
    #[cfg(not(target_os = "windows"))]
    std::process::Command::new("zenity")
        .args([
            "--file-selection",
            "--title=Escolha uma imagem",
            "--file-filter=Imagens | *.png *.jpg *.jpeg *.webp",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
}

/// O seletor de áudio dos formatos aceitos pelo soundboard do servidor.
pub fn pick_audio() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    return crate::shell::pick_audio();

    #[cfg(not(target_os = "windows"))]
    std::process::Command::new("zenity")
        .args([
            "--file-selection",
            "--title=Escolha um som",
            "--file-filter=Áudio | *.mp3 *.ogg *.wav",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
}

/// Instalado pela Microsoft Store, quem atualiza é a Store: o app não procura versão no site. O
/// beta também não: o instalador do site é o do estável, e trocaria o app instalado.
pub fn updated_elsewhere() -> bool {
    if crate::BETA {
        return true;
    }

    #[cfg(target_os = "windows")]
    return crate::shell::packaged();

    #[cfg(not(target_os = "windows"))]
    false
}

/// Há versão nova? Baixa com o progresso na tela, confere a assinatura e entrega ao instalador.
/// `true` quando o app está de saída. Falhou em qualquer ponto, abre na versão que tem:
/// atualizar nunca impede de usar.
///
/// ponytail: só na abertura e pelo aviso do tempo real; quem passa dias com o app aberto sem
/// o servidor avisar fica na versão. A saída é perguntar ao site de tempos em tempos.
pub async fn updating(api: &Api, courier: &Courier) -> bool {
    if updated_elsewhere() {
        return false;
    }

    courier.send(Event::Status("Procurando atualizações…".to_owned()));

    let Some(release) = api.newer_release(core_app::update::PLATFORM).await else {
        return false;
    };
    let mut said = String::new();
    let installer = core_app::update::fetch(api, &release, |downloaded, total| {
        let status = match total.filter(|total| *total > 0) {
            Some(total) => format!(
                "Baixando a atualização… {}%",
                (downloaded * 100 / total).min(100)
            ),
            None => format!(
                "Baixando a atualização… {:.1} MB",
                downloaded as f64 / 1024.0 / 1024.0
            ),
        };

        // Um pedaço chega a cada poucos KB: avisar só quando o texto muda.
        if status != said {
            said.clone_from(&status);
            courier.send(Event::Status(status));
        }
    })
    .await;

    let Some(installer) = installer else {
        return false;
    };

    courier.send(Event::Status(format!(
        "Instalando a versão {}…",
        release.version
    )));

    // Na abertura, escondida quando veio do logon: a versão nova volta do mesmo jeito.
    install(
        &installer,
        std::env::args().any(|argument| argument == "--background"),
    )
}

/// Abre o instalador e sai: `/P` sem perguntas, `/UPDATE` é troca e não instalação nova, `/R`
/// reabre o app no fim. Com o app escondido na bandeja a troca é toda silenciosa (`/S`) e ele
/// volta do mesmo jeito (`/BACKGROUND`).
#[cfg(target_os = "windows")]
pub fn install(installer: &std::path::Path, hidden: bool) -> bool {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    let file = HSTRING::from(installer.as_os_str());
    let parameters = if hidden {
        w!("/S /UPDATE /R /BACKGROUND")
    } else {
        w!("/P /UPDATE /R")
    };
    // SAFETY: as cadeias vivem até o fim da chamada, que não guarda nenhuma delas.
    let opened = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &file,
            parameters,
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    // Acima de 32 é sucesso: é assim que o ShellExecute responde desde sempre.
    if opened.0 as isize <= 32 {
        tracing::warn!(
            code = opened.0 as isize,
            "atualização: o instalador não abriu"
        );

        return false;
    }

    std::process::exit(0);
}

/// No Linux quem atualiza é o APT.
#[cfg(not(target_os = "windows"))]
pub fn install(_installer: &std::path::Path, _hidden: bool) -> bool {
    false
}

/// Abre um endereço no navegador da pessoa sem o administrador do app. Quem abre é o Explorer
/// da área de trabalho, que roda sem elevação: abrir direto daria um navegador elevado, que
/// briga com o perfil do navegador já aberto.
#[cfg(target_os = "windows")]
pub fn open_in_browser(url: &str) {
    let url = url.to_owned();
    let spawned = std::thread::Builder::new().name("navegador".into()).spawn(move || {
        if let Err(failure) = open_through_desktop(&url) {
            tracing::warn!(%failure, "navegador: a área de trabalho não abriu o endereço, abrindo direto");
            open_directly(&url);
        }
    });

    if let Err(failure) = spawned {
        tracing::warn!(%failure, "navegador: a thread não subiu");
    }
}

/// No Linux quem sabe qual é o navegador da pessoa é o `xdg-open`.
#[cfg(not(target_os = "windows"))]
pub fn open_in_browser(url: &str) {
    if let Err(failure) = std::process::Command::new("xdg-open").arg(url).spawn() {
        tracing::warn!(%failure, "navegador: o xdg-open não abriu");
    }
}

/// O COM desta thread em volta do pedido ao shell da área de trabalho.
#[cfg(target_os = "windows")]
fn open_through_desktop(url: &str) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

    // SAFETY: COM de apartamento único nesta thread, só dela, e desfeito antes de ela acabar.
    unsafe {
        let started = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let opened = desktop_shell_execute(url);

        if started.is_ok() {
            CoUninitialize();
        }

        opened
    }
}

/// O caminho do shell da área de trabalho: a janela do desktop, o navegador de pastas dela, a
/// vista e, por fim, o objeto de automação do Explorer, que executa como o próprio Explorer.
#[cfg(target_os = "windows")]
fn desktop_shell_execute(url: &str) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{
        CLSCTX_LOCAL_SERVER, CoCreateInstance, IDispatch, IServiceProvider,
    };
    use windows::Win32::System::Variant::{VARIANT, VT_I4};
    use windows::Win32::UI::Shell::{
        IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellView, IShellWindows,
        SID_STopLevelBrowser, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
    };
    use windows::core::{BSTR, Interface};

    // SAFETY: chamadas COM com o COM já aberto nesta thread; o `VARIANT` do desktop é o
    // `CSIDL_DESKTOP` (zero) marcado como inteiro, com o resto zerado pelo `default`.
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let mut desktop = VARIANT::default();

        (*desktop.Anonymous.Anonymous).vt = VT_I4;

        let mut window = 0;
        let found = windows.FindWindowSW(
            &desktop,
            &VARIANT::default(),
            SWC_DESKTOP,
            &mut window,
            SWFO_NEEDDISPATCH,
        )?;
        let browser: IShellBrowser = found
            .cast::<IServiceProvider>()?
            .QueryService(&SID_STopLevelBrowser)?;
        let view: IShellView = browser.QueryActiveShellView()?;
        let background: IDispatch = view.GetItemObject(SVGIO_BACKGROUND)?;
        let shell: IShellDispatch2 = background
            .cast::<IShellFolderViewDual>()?
            .Application()?
            .cast()?;

        shell.ShellExecute(
            &BSTR::from(url),
            &VARIANT::default(),
            &VARIANT::default(),
            &VARIANT::default(),
            &VARIANT::default(),
        )
    }
}

/// O último recurso: o navegador sai com o nível do app, mas o login não fica sem navegador.
#[cfg(target_os = "windows")]
fn open_directly(url: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    // SAFETY: as duas cadeias vivem até o fim da chamada, que não guarda nenhuma delas.
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(test)]
mod tests {
    /// Abre uma aba de verdade, pelo Explorer da área de trabalho: é o caminho do login com o
    /// Google. `cargo test -p unkvoid -- --ignored desktop_opens`.
    #[test]
    #[ignore]
    #[cfg(target_os = "windows")]
    fn the_desktop_opens_a_page_in_the_browser() {
        super::open_through_desktop("https://unkvoid.com")
            .expect("o Explorer da área de trabalho abriu o endereço");
    }

    #[test]
    fn the_device_name_says_the_system() {
        assert!(super::device_name().starts_with(std::env::consts::OS));
    }
}
