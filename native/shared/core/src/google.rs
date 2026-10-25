//! Entrar com o Google num app que não é navegador.
//!
//! O app abre uma porta em `127.0.0.1`, manda a pessoa ao navegador com a porta, um `state`
//! sorteado e o challenge S256 (`GET /oauth2/app`). O callback devolve apenas um código à porta;
//! o app o troca por tokens no corpo de um POST usando o verifier que não saiu do processo.
//! O `state` de volta tem de ser o que saiu: sem isso qualquer página aberta na máquina poderia
//! tentar completar o login local.
//!
//! É o caminho da porta, e não o de um esquema `unkvoid://`, porque funciona igual nos três
//! sistemas e não depende de o app estar instalado e registrado.

use std::time::Duration;

use anyhow::{Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Quanto se espera a pessoa escolher a conta no navegador.
const PATIENCE: Duration = Duration::from_secs(300);

/// A aba tenta se fechar. O navegador só deixa quando ela tem uma página só no histórico —
/// com a escolha de conta do Google no meio ele recusa, e a frase fica.
const DONE: &str = "<!doctype html><meta charset=utf-8><title>Unkvoid</title>\
<body style=\"font-family:system-ui;background:#06050a;color:#ece9f3;display:grid;place-items:center;height:100vh;margin:0\">\
<p>Pronto, você entrou. Pode fechar esta aba e voltar para o Unkvoid.</p>\
<script>window.close()</script>";

pub struct GoogleLogin {
    listener: TcpListener,
    state: String,
    verifier: String,
    server: String,
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
        let verifier = URL_SAFE_NO_PAD.encode(rand::thread_rng().r#gen::<[u8; 32]>());
        let challenge = code_challenge(&verifier);
        let server = server.trim_end_matches('/').to_owned();
        let url = format!(
            "{server}/oauth2/app?port={port}&state={state}&refresh=1&code_challenge={challenge}&code_challenge_method=S256"
        );

        Ok(Self {
            listener,
            state,
            verifier,
            server,
            url,
        })
    }

    /// Espera o navegador voltar e devolve o token e o de renovação. Pedido com o `state`
    /// errado é ignorado, e a espera continua: pode ser outra coisa na máquina batendo na porta.
    pub async fn wait(self) -> Result<(String, Option<String>)> {
        tokio::time::timeout(PATIENCE, async {
            loop {
                let (mut browser, _) = self.listener.accept().await?;
                let mut request = [0_u8; 4096];
                let size = browser.read(&mut request).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..size]);
                let mut exchange_failed = false;
                let credentials = if let Some(code) = code_of(&request, &self.state) {
                    match self.exchange(&code).await {
                        Ok(credentials) => Some(credentials),
                        Err(_) => {
                            exchange_failed = true;
                            None
                        }
                    }
                } else {
                    None
                };

                let (status, body) = if credentials.is_some() { ("200 OK", DONE) } else { ("400 Bad Request", "") };
                let reply = format!("HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());

                let _ = browser.write_all(reply.as_bytes()).await;

                if let Some(credentials) = credentials {
                    return Ok(credentials);
                }
                if exchange_failed {
                    return Err(anyhow!("o login do Google não foi concluído"));
                }
            }
        })
        .await
        .map_err(|_| anyhow!("ninguém voltou do navegador"))?
    }
}

impl GoogleLogin {
    async fn exchange(&self, code: &str) -> Result<(String, Option<String>)> {
        let answer = reqwest::Client::new()
            .post(format!("{}/api/auth/app/exchange", self.server))
            .timeout(Duration::from_secs(10))
            .json(&serde_json::json!({ "code": code, "code_verifier": self.verifier }))
            .send()
            .await
            .map_err(|_| anyhow!("o código de entrada não foi aceito"))?;
        if !answer.status().is_success() {
            return Err(anyhow!("o código de entrada não foi aceito"));
        }
        let body: serde_json::Value = answer
            .json()
            .await
            .map_err(|_| anyhow!("a resposta do login não pôde ser lida"))?;
        let token = body["data"]["token"]
            .as_str()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| anyhow!("a resposta do login não trouxe a sessão"))?
            .to_owned();
        let refresh = body["data"]["refresh_token"].as_str().map(str::to_owned);

        Ok((token, refresh))
    }
}

pub fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn code_of(request: &str, expected: &str) -> Option<String> {
    let target = request.lines().next()?.split_whitespace().nth(1)?;
    let query = target.split_once('?')?.1;
    let value = |name: &str| {
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
    };

    if value("state")? != expected {
        return None;
    }

    let code = decode(value("code")?);

    (!code.is_empty()).then_some(code)
}

/// O código gerado pelo Laravel usa caracteres seguros; a decodificação também aceita escape URL.
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
    fn only_a_code_with_the_matching_state_is_accepted() {
        let request = "GET /?code=12_abc&state=f00d HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";

        assert_eq!(code_of(request, "f00d"), Some("12_abc".to_owned()));
        assert_eq!(
            code_of("GET /?code=abc&state=beef HTTP/1.1\r\n\r\n", "f00d"),
            None
        );
        assert_eq!(code_of("GET /favicon.ico HTTP/1.1\r\n\r\n", "f00d"), None);
        assert_eq!(
            code_of("GET /?state=f00d&token=plain HTTP/1.1\r\n\r\n", "f00d"),
            None
        );
    }

    #[test]
    fn creates_the_rfc_7636_appendix_b_challenge() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

        assert_eq!(
            code_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn reads_only_a_code_with_the_matching_state() {
        assert_eq!(
            code_of("GET /?code=abc_123&state=f00d HTTP/1.1", "f00d"),
            Some("abc_123".to_owned())
        );
        assert_eq!(code_of("GET /?code=abc&state=beef HTTP/1.1", "f00d"), None);
    }

    #[tokio::test]
    async fn the_login_url_carries_only_the_public_pkce_challenge() {
        let login = GoogleLogin::start("http://127.0.0.1:8000/")
            .await
            .expect("start");

        assert!(
            login
                .url
                .starts_with("http://127.0.0.1:8000/oauth2/app?port="),
            "{}",
            login.url
        );
        assert!(login.url.contains("code_challenge_method=S256"));
        assert!(login.url.contains(&format!(
            "code_challenge={}",
            code_challenge(&login.verifier)
        )));
    }
}
