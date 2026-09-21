//! O cliente da API do Laravel.
//!
//! Tudo o que precisa de conta passa por aqui: entrar, os servidores, a árvore de um
//! servidor, as mensagens, e o token de 60 s que abre a voz. A mídia não — ela vai direto
//! ao SFU.
//!
//! **O que chega à tela.** Erro de validação o Laravel já devolve em português e falando do
//! que a pessoa digitou ("este e-mail já está em uso"): esse texto é útil e passa. Qualquer
//! outra falha vira um [`Failure`], sem caminho, endereço nem código de status — ver
//! `failure.rs`.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::failure::Failure;
use crate::models::{AuthToken, Config, Message, ServerSummary, ServerTree, User};

/// Dez segundos, o mesmo do app de hoje. Passar disto a pessoa já desistiu e clicou de novo.
const TIMEOUT: Duration = Duration::from_secs(10);

/// O que a chamada devolve quando falha: ou um motivo que a interface traduz, ou o texto de
/// validação que o Laravel escreveu para quem está digitando.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    Failed(Failure),
    /// Já em português e já sobre o campo errado. É o único texto do servidor que a tela
    /// mostra — e vem com o nome do campo, para a tela pintar o input certo de vermelho e
    /// pôr a frase embaixo dele, e não numa linha solta no rodapé.
    Invalid { field: String, message: String },
}

impl From<Failure> for HttpError {
    fn from(failure: Failure) -> Self {
        Self::Failed(failure)
    }
}

pub struct Api {
    base: String,
    token: Mutex<Option<String>>,
    http: reqwest::Client,
}

impl Api {
    pub fn new(base: &str) -> anyhow::Result<Self> {
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            token: Mutex::new(None),
            http: reqwest::Client::builder().timeout(TIMEOUT).build()?,
        })
    }

    pub fn signed_in(&self) -> bool {
        self.token().is_some()
    }

    /// O servidor responde? É o `/health`, que existe para esta pergunta e não pede conta.
    pub async fn reachable(&self) -> bool {
        self.http.get(self.url("/health")).send().await.is_ok_and(|answer| answer.status().is_success())
    }

    pub async fn register(&self, email: &str, password: &str, device: &str) -> Result<AuthToken, HttpError> {
        self.authenticate("/api/auth/register", email, password, device).await
    }

    /// A árvore inteira do servidor: cargos, canais, membros e quem está em cada voz.
    pub async fn tree(&self, server: i64) -> Result<ServerTree, HttpError> {
        self.get(&format!("/api/servers/{server}")).await
    }

    /// O token de quem tem conta para entrar numa sala por código.
    pub async fn room_token(&self, room: &str) -> Result<String, HttpError> {
        self.token_from(&format!("/api/rooms/{room}/token")).await
    }

    pub fn set_token(&self, token: Option<String>) {
        *self.token.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = token;
    }

    pub fn token(&self) -> Option<String> {
        self.token.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    pub async fn config(&self) -> Result<Config, HttpError> {
        self.get("/api/config").await
    }

    pub async fn login(&self, email: &str, password: &str, device: &str) -> Result<AuthToken, HttpError> {
        self.authenticate("/api/auth/login", email, password, device).await
    }

    /// Entrar e criar conta só diferem no caminho: as duas devolvem o token e já o guardam,
    /// para a próxima chamada não precisar lembrar de passá-lo.
    async fn authenticate(&self, path: &str, email: &str, password: &str, device: &str) -> Result<AuthToken, HttpError> {
        let answer: AuthToken = self
            .post(path, &serde_json::json!({ "email": email, "password": password, "device": device }))
            .await?;

        self.set_token(Some(answer.token.clone()));

        Ok(answer)
    }

    pub async fn me(&self) -> Result<User, HttpError> {
        self.get("/api/me").await
    }

    pub async fn servers(&self) -> Result<Vec<ServerSummary>, HttpError> {
        self.get("/api/servers").await
    }

    pub async fn messages(&self, channel: &str) -> Result<Vec<Message>, HttpError> {
        self.get(&format!("/api/channels/{channel}/messages")).await
    }

    pub async fn send_message(&self, channel: &str, body: &str) -> Result<Message, HttpError> {
        self.post(&format!("/api/channels/{channel}/messages"), &serde_json::json!({ "body": body }))
            .await
    }

    /// O token que identifica o socket no tempo real. Vale 60 s: pedir um novo a cada
    /// conexão é mais simples do que guardar e conferir validade.
    pub async fn realtime_token(&self) -> Result<String, HttpError> {
        self.token_from("/api/sfu/session").await
    }

    async fn token_from(&self, path: &str) -> Result<String, HttpError> {
        let answer: Value = self.post(path, &serde_json::json!({})).await?;

        answer["token"]
            .as_str()
            .map(str::to_owned)
            .ok_or(HttpError::Failed(Failure::ServerBroke))
    }

    /// O token de voz de um canal, que diz o que esta sessão pode produzir.
    pub async fn voice_token(&self, channel: &str) -> Result<String, HttpError> {
        self.token_from(&format!("/api/channels/{channel}/voice/token")).await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, HttpError> {
        self.send(self.http.get(self.url(path)), path).await
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: &impl Serialize) -> Result<T, HttpError> {
        self.send(self.http.post(self.url(path)).json(body), path).await
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn send<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
        path: &str,
    ) -> Result<T, HttpError> {
        let request = request.header("accept", "application/json");

        let request = match self.token() {
            Some(token) => request.bearer_auth(token),
            None => request,
        };

        let response = match request.send().await {
            Ok(response) => response,
            Err(failure) => {
                tracing::warn!(%failure, path, "a chamada não chegou ao servidor");

                return Err(Failure::Unreachable.into());
            }
        };

        let status = response.status().as_u16();
        let body: Value = response.json().await.unwrap_or(Value::Null);

        if !(200..300).contains(&status) {
            return Err(refusal(status, &body, path));
        }

        let payload = match body.as_object() {
            Some(object) if object.len() == 1 && object.contains_key("data") => body["data"].clone(),
            _ => body,
        };

        serde_json::from_value(payload).map_err(|failure| {
            tracing::warn!(%failure, path, "a resposta não tem o formato esperado");

            Failure::ServerBroke.into()
        })
    }
}

/// Erro de validação vira o texto que o Laravel escreveu; o resto vira motivo, e o detalhe
/// fica no log.
fn refusal(status: u16, body: &Value, path: &str) -> HttpError {
    if status == 422
        && let Some((field, message)) = first_field_error(body)
    {
        return HttpError::Invalid { field, message };
    }

    tracing::warn!(status, path, message = %body["message"], "o servidor recusou");

    HttpError::Failed(Failure::from_status(status))
}

/// O primeiro campo com erro, com o nome junto. O Laravel devolve
/// `{"errors": {"email": ["..."]}}`, e é o `email` dali que diz qual input errou.
fn first_field_error(body: &Value) -> Option<(String, String)> {
    body["errors"].as_object()?.iter().find_map(|(field, messages)| {
        let message = messages.as_array()?.first()?.as_str()?;

        Some((field.clone(), message.to_owned()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    #[test]
    fn a_validation_error_keeps_the_sentence_the_person_needs_to_read() {
        let body = json!({
            "message": "The given data was invalid.",
            "errors": { "email": ["Este e-mail já está em uso."] },
        });

        assert_eq!(
            refusal(422, &body, "/api/auth/register"),
            HttpError::Invalid {
                field: "email".into(),
                message: "Este e-mail já está em uso.".into(),
            },
            "perdeu o nome do campo: a tela não sabe qual input pintar",
        );
    }

    #[test]
    fn every_other_refusal_becomes_a_reason_and_leaks_nothing() {
        let body = json!({ "message": "Server Error at /api/servers/7 (SQLSTATE[42S02])" });
        let refused = refusal(500, &body, "/api/servers/7");

        assert_eq!(refused, HttpError::Failed(Failure::ServerBroke));

        let HttpError::Failed(failure) = refused else {
            panic!("virou texto em vez de motivo");
        };
        let json = serde_json::to_string(&failure).expect("serialize");

        assert!(!json.contains("SQLSTATE"), "vazou o banco: {json}");
        assert!(!json.contains("api"), "vazou o caminho: {json}");
    }

    #[test]
    fn a_422_without_field_errors_still_does_not_leak() {
        let body = json!({ "message": "The given data was invalid." });

        assert_eq!(refusal(422, &body, "/api/x"), HttpError::Failed(Failure::Invalid));
    }

    #[test]
    fn the_base_address_never_ends_up_with_two_slashes() {
        assert_eq!(Api::new("http://127.0.0.1:8000/").expect("build").url("/api/config"), "http://127.0.0.1:8000/api/config");
        assert_eq!(Api::new("http://127.0.0.1:8000").expect("build").url("/api/config"), "http://127.0.0.1:8000/api/config");
    }

    #[test]
    fn the_token_is_remembered_and_can_be_dropped() {
        let api = Api::new("http://127.0.0.1:8000").expect("build");

        assert!(api.token().is_none());

        api.set_token(Some("1|abc".into()));

        assert_eq!(api.token().as_deref(), Some("1|abc"));

        api.set_token(None);

        assert!(api.token().is_none(), "sair não esqueceu o token");
    }
}
