//! Entrar com o Google: o navegador do sistema faz o login e devolve o token pelo
//! esquema `unkvoid://`, que o sistema operacional entrega a esta janela.
//!
//! O token viaja dentro do endereço, então o `state` sorteado aqui tem de voltar igual:
//! é o que impede uma página qualquer de mandar um token e logar a pessoa numa conta
//! alheia.
//!
//! ponytail: quem registrar o mesmo esquema na máquina pode interceptar o retorno. A
//! porta local de antes não tinha esse risco, mas exigia um servidor HTTP dentro do app
//! e uma tela de navegador feia no fim. Se a interceptação passar a importar, o caminho
//! é PKCE, com o segredo nascendo aqui dentro e o token sendo trocado por ele.

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use url::Url;

/// Quanto o navegador tem para voltar. Dois minutos dá para digitar a senha e o segundo
/// fator; depois disso a pessoa já desistiu e a espera não precisa continuar.
const DEADLINE_SECONDS: u64 = 120;

/// Quem está esperando o retorno: o `state` sorteado e por onde entregar o token. Um
/// pedido novo toma o lugar do anterior, e o anterior morre na espera — sem isto dois
/// cliques deixariam dois esperando, e o token cairia no que ninguém mais ouve.
static PENDING: Mutex<Option<(String, Sender<String>)>> = Mutex::new(None);

fn pending() -> std::sync::MutexGuard<'static, Option<(String, Sender<String>)>> {
    PENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[tauri::command]
pub async fn google_login(server: String) -> Result<String, String> {
    // A mesma trava do `open_url`: o endereço vira URL no navegador do sistema, e no
    // Windows um `file://` abriria um arquivo remoto.
    if ! is_web_url(&server) {
        return Err("só endereços http e https".into());
    }

    let state: String = (0..16).map(|_| format!("{:02x}", rand::random::<u8>())).collect();
    let (sender, receiver) = channel();

    *pending() = Some((state.clone(), sender));

    open_in_browser(&format!("{}/oauth2/app?state={state}", server.trim_end_matches('/')))
        .map_err(|error| error.to_string())?;

    tokio::task::spawn_blocking(move || {
        receiver
            .recv_timeout(Duration::from_secs(DEADLINE_SECONDS))
            .map_err(|_| "o navegador não voltou com o token em dois minutos".to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// O sistema entregou um `unkvoid://…` ao app. Só o retorno do login interessa, e só
/// quando o `state` é o que este processo sorteou.
pub fn handle_deep_link(url: &Url) {
    // Sem a query no log: ela leva o token. O que importa para achar o defeito é se o
    // endereço chegou a esta janela e por que foi recusado.
    tracing::info!(host = url.host_str(), path = url.path(), "login: endereço do sistema chegou");

    let mut slot = pending();

    let Some(state) = slot.as_ref().map(|(state, _)| state.clone()) else {
        tracing::warn!("login: ninguém esperando o retorno do navegador");
        return;
    };

    let Some(token) = token_from_url(url, Some(&state)) else {
        tracing::warn!("login: retorno sem token ou com outro state");
        return;
    };

    if let Some((_, sender)) = slot.take() {
        let _ = sender.send(token);
    }
}

/// O token de `unkvoid://login?token=…&state=…`, já sem a codificação de URL, e só se o
/// `state` bater. `None` para qualquer outro endereço.
fn token_from_url(url: &Url, expected_state: Option<&str>) -> Option<String> {
    if url.scheme() != "unkvoid" || url.host_str() != Some("login") {
        return None;
    }

    let value = |name: &str| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned());

    if value("state")? != expected_state? {
        return None;
    }

    value("token").filter(|token| ! token.is_empty())
}

/// Abre um endereço no navegador do sistema: baixar clipe passa por aqui, porque a janela
/// do Tauri não baixa com confiança um `<a download>` de outra origem. Só `http` e
/// `https`: no Windows o `FileProtocolHandler` também abriria um caminho local.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if ! is_web_url(&url) {
        return Err("só endereços http e https".into());
    }

    open_in_browser(&url).map_err(|error| error.to_string())
}

fn is_web_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
}

/// O navegador padrão, pelo que cada sistema já tem. Sem plugin: é uma chamada só.
fn open_in_browser(url: &str) -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    command.stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_comes_out_of_the_link_only_with_the_right_state() {
        let link = |texto: &str| Url::parse(texto).expect("endereço inválido");

        assert_eq!(token_from_url(&link("unkvoid://login?token=12%7Cab%2Bcd&state=abc"), Some("abc")).as_deref(), Some("12|ab+cd"));
        assert_eq!(token_from_url(&link("unkvoid://login/?token=12%7Cab&state=abc"), Some("abc")).as_deref(), Some("12|ab"), "o Windows acrescenta a barra");
        assert_eq!(token_from_url(&link("unkvoid://login?token=12&state=xyz"), Some("abc")), None);
        assert_eq!(token_from_url(&link("unkvoid://login?token=12"), Some("abc")), None);
        assert_eq!(token_from_url(&link("unkvoid://login?token=&state=abc"), Some("abc")), None);
        assert_eq!(token_from_url(&link("unkvoid://outro?token=12&state=abc"), Some("abc")), None, "só o retorno do login");
        assert_eq!(token_from_url(&link("https://unkvoid.com/login?token=12&state=abc"), Some("abc")), None, "endereço da web não é retorno");
        assert_eq!(token_from_url(&link("unkvoid://login?token=12&state=abc"), None), None, "ninguém esperando, nada entra");
    }

    #[test]
    fn only_web_addresses_reach_the_browser() {
        assert!(is_web_url("https://minio.example/clips/01j8/clip.mp4?X-Amz-Signature=abc&response-content-disposition=attachment"));
        assert!(is_web_url("http://127.0.0.1:9000/clip.mp4"));
        assert!(! is_web_url("file:///C:/Windows/System32/calc.exe"));
        assert!(! is_web_url("C:\\Windows\\System32\\calc.exe"));
        assert!(! is_web_url("javascript:alert(1)"));
        assert!(! is_web_url(""));
    }
}
