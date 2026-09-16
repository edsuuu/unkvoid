//! Entrar com o Google: o navegador do sistema faz o login e volta com o token do
//! Sanctum para uma porta local que só existe durante a espera.

use std::sync::atomic::{AtomicU64, Ordering};

/// Qual pedido de login é o atual. Um clique novo (ou o diálogo fechado e reaberto)
/// avança o contador, e a espera antiga percebe e sai — sem isto ficavam dois
/// servidores locais abertos, e o token do navegador podia cair no que ninguém mais
/// ouvia.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Quanto o navegador tem para voltar. Dois minutos dá para digitar a senha e o segundo
/// fator; depois disso a pessoa já desistiu e a porta não precisa continuar aberta.
const DEADLINE_SECONDS: u64 = 120;

#[tauri::command]
pub async fn google_login(server: String) -> Result<String, String> {
    // A mesma trava do `open_url`: o endereço vira URL no navegador do sistema, e no
    // Windows um `file://` abriria um arquivo remoto.
    if ! is_web_url(&server) {
        return Err("só endereços http e https".into());
    }

    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;

    tokio::task::spawn_blocking(move || login_loopback(&server, generation).map_err(|error| error.to_string()))
        .await
        .map_err(|error| error.to_string())?
}

/// Espera o navegador chamar `http://127.0.0.1:<porta>/?token=…&state=…`.
///
/// O `state` é sorteado aqui e tem de voltar igual: sem ele qualquer página aberta na
/// máquina poderia mandar um token para esta porta e logar a pessoa numa conta alheia.
fn login_loopback(server: &str, generation: u64) -> anyhow::Result<String> {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let deadline = Instant::now() + Duration::from_secs(DEADLINE_SECONDS);
    let state: String = (0..16).map(|_| format!("{:02x}", rand::random::<u8>())).collect();

    listener.set_nonblocking(true)?;
    open_in_browser(&format!("{}/oauth2/app?port={port}&state={state}", server.trim_end_matches('/')))?;

    while Instant::now() < deadline {
        if GENERATION.load(Ordering::Relaxed) != generation {
            anyhow::bail!("login cancelado por um pedido mais novo");
        }

        let Ok((mut stream, _)) = listener.accept() else {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };

        let mut request = [0_u8; 4096];
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let read = stream.read(&mut request).unwrap_or(0);

        let Some(token) = token_from_request(&String::from_utf8_lossy(&request[..read]), &state) else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
            continue;
        };

        let body = "<!doctype html><meta charset=\"utf-8\"><title>Unkvoid</title>\
            <body style=\"font-family:sans-serif;text-align:center;padding:4rem\">\
            <h1>Pronto</h1><p>Pode voltar para o Unkvoid.</p></body>";

        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        );

        return Ok(token);
    }

    Err(anyhow::anyhow!("o navegador não voltou com o token em dois minutos"))
}

/// O token de `GET /?token=…&state=… HTTP/1.1`: a segunda palavra da primeira linha, já
/// sem a codificação de URL, e só se o `state` for o esperado. `None` para qualquer
/// outro pedido (o `favicon.ico`, por exemplo).
fn token_from_request(head: &str, expected_state: &str) -> Option<String> {
    let path = head.lines().next()?.split_whitespace().nth(1)?;
    let url = url::Url::parse(&format!("http://127.0.0.1{path}")).ok()?;
    let value = |name: &str| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned());

    if value("state")? != expected_state {
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
    fn the_token_comes_out_of_the_first_line_decoded_only_with_the_right_state() {
        assert_eq!(
            token_from_request("GET /?token=12%7Cab%2Bcd&state=abc HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", "abc").as_deref(),
            Some("12|ab+cd")
        );
        assert_eq!(token_from_request("GET /?token=12&state=xyz HTTP/1.1\r\n", "abc"), None);
        assert_eq!(token_from_request("GET /?token=12 HTTP/1.1\r\n", "abc"), None);
        assert_eq!(token_from_request("GET /favicon.ico HTTP/1.1\r\n", "abc"), None);
        assert_eq!(token_from_request("GET /?token=&state=abc HTTP/1.1\r\n", "abc"), None);
        assert_eq!(token_from_request("", "abc"), None);
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
