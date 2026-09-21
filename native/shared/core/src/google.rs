//! Entrar com o Google num app que não é navegador.
//!
//! O app abre uma porta em `127.0.0.1`, manda a pessoa ao navegador com a porta e um `state`
//! sorteado (`GET /oauth2/app`), e o Laravel, depois do Google, devolve o navegador para essa
//! porta com o token do Sanctum. O `state` de volta tem de ser o que saiu: sem isso qualquer
//! página aberta na máquina entregaria um token ao app.
//!
//! É o caminho da porta, e não o do esquema `unkvoid://`, porque funciona igual nos três
//! sistemas e não depende de o app estar instalado e registrado.

use std::time::Duration;

use anyhow::{Result, anyhow};
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Quanto se espera a pessoa escolher a conta no navegador.
const PATIENCE: Duration = Duration::from_secs(300);

const DONE: &str = "<!doctype html><meta charset=utf-8><title>Unkvoid</title>\
<body style=\"font-family:system-ui;background:#06050a;color:#ece9f3;display:grid;place-items:center;height:100vh;margin:0\">\
<p>Pronto. Pode fechar esta aba e voltar para o Unkvoid.</p>";

pub struct GoogleLogin {
    listener: TcpListener,
    state: String,
    /// O endereço que a interface abre no navegador da pessoa.
    pub url: String,
}

impl GoogleLogin {
    pub async fn start(server: &str) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let state: String = (0..32)
            .map(|_| format!("{:x}", rand::thread_rng().gen_range(0..16)))
            .collect();
        let url = format!(
            "{}/oauth2/app?port={port}&state={state}",
            server.trim_end_matches('/')
        );

        Ok(Self {
            listener,
            state,
            url,
        })
    }

    /// Espera o navegador voltar e devolve o token. Pedido com o `state` errado é ignorado, e
    /// a espera continua: pode ser outra coisa na máquina batendo na porta.
    pub async fn wait(self) -> Result<String> {
        tokio::time::timeout(PATIENCE, async {
            loop {
                let (mut browser, _) = self.listener.accept().await?;
                let mut request = [0_u8; 4096];
                let size = browser.read(&mut request).await.unwrap_or(0);
                let token = token_of(&String::from_utf8_lossy(&request[..size]), &self.state);

                let (status, body) = if token.is_some() { ("200 OK", DONE) } else { ("400 Bad Request", "") };
                let reply = format!("HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());

                let _ = browser.write_all(reply.as_bytes()).await;

                if let Some(token) = token {
                    return Ok(token);
                }
            }
        })
        .await
        .map_err(|_| anyhow!("ninguém voltou do navegador"))?
    }
}

/// O token da primeira linha do pedido (`GET /?token=…&state=… HTTP/1.1`), se o `state` bate.
fn token_of(request: &str, expected: &str) -> Option<String> {
    let query = request
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .split_once('?')?
        .1;
    let field = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
    };

    if field("state")? != expected {
        return None;
    }

    let token = decode(field("token")?);

    (!token.is_empty()).then_some(token)
}

/// O token do Sanctum vem com `|`, que o Laravel manda como `%7C`.
fn decode(text: &str) -> String {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();

    while let [first, tail @ ..] = rest {
        match (first, tail) {
            (b'%', [high, low, after @ ..])
                if let Some(byte) =
                    hex(*high).zip(hex(*low)).map(|(high, low)| high * 16 + low) =>
            {
                bytes.push(byte);
                rest = after;
            }
            _ => {
                bytes.push(*first);
                rest = tail;
            }
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

fn hex(digit: u8) -> Option<u8> {
    char::from(digit).to_digit(16).map(|value| value as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_read_only_when_the_state_matches() {
        let request = "GET /?token=12%7Cabc&state=f00d HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";

        assert_eq!(token_of(request, "f00d").as_deref(), Some("12|abc"));
        assert_eq!(
            token_of(request, "beef"),
            None,
            "o state de outra pessoa não entrega token"
        );
        assert_eq!(token_of("GET /favicon.ico HTTP/1.1\r\n\r\n", "f00d"), None);
        assert_eq!(
            token_of("GET /?state=f00d&token= HTTP/1.1\r\n\r\n", "f00d"),
            None,
            "token vazio não é token"
        );
    }

    #[tokio::test]
    async fn the_browser_coming_back_hands_the_token_over() {
        let login = GoogleLogin::start("http://127.0.0.1:8000/")
            .await
            .expect("start");
        let port = login.listener.local_addr().expect("address").port();
        let state = login.state.clone();

        assert!(
            login
                .url
                .starts_with("http://127.0.0.1:8000/oauth2/app?port="),
            "{}",
            login.url
        );

        let waiting = tokio::spawn(login.wait());

        for path in [
            "/?token=wrong&state=0000".to_owned(),
            format!("/?token=9%7Cxyz&state={state}"),
        ] {
            let mut browser = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("connect");

            browser
                .write_all(format!("GET {path} HTTP/1.1\r\n\r\n").as_bytes())
                .await
                .expect("request");

            let mut answer = String::new();
            let _ = browser.read_to_string(&mut answer).await;
        }

        assert_eq!(waiting.await.expect("join").expect("token"), "9|xyz");
    }
}
