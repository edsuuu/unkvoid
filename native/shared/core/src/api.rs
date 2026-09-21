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
use crate::models::{
    AuthToken, ChannelKind, Config, Conversation, DirectMessage, Friendship, Message, ServerSummary,
    ServerTree, User,
};

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
    Invalid {
        field: String,
        message: String,
    },
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
        self.http
            .get(self.url("/health"))
            .send()
            .await
            .is_ok_and(|answer| answer.status().is_success())
    }

    pub async fn register(
        &self,
        email: &str,
        password: &str,
        device: &str,
    ) -> Result<AuthToken, HttpError> {
        self.authenticate("/api/auth/register", email, password, device)
            .await
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
        *self
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = token;
    }

    pub fn token(&self) -> Option<String> {
        self.token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub async fn config(&self) -> Result<Config, HttpError> {
        self.get("/api/config").await
    }

    pub async fn login(
        &self,
        email: &str,
        password: &str,
        device: &str,
    ) -> Result<AuthToken, HttpError> {
        self.authenticate("/api/auth/login", email, password, device)
            .await
    }

    /// Entrar e criar conta só diferem no caminho: as duas devolvem o token e já o guardam,
    /// para a próxima chamada não precisar lembrar de passá-lo.
    async fn authenticate(
        &self,
        path: &str,
        email: &str,
        password: &str,
        device: &str,
    ) -> Result<AuthToken, HttpError> {
        let answer: AuthToken = self
            .post(
                path,
                &serde_json::json!({ "email": email, "password": password, "device": device }),
            )
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

    /// Cria um servidor. Ele já nasce com um canal de texto e um de voz — quem decide isso
    /// é o Laravel, e é por isso que a resposta já serve para abrir a árvore.
    pub async fn create_server(&self, name: &str) -> Result<ServerSummary, HttpError> {
        self.post("/api/servers", &serde_json::json!({ "name": name })).await
    }

    /// Entra num servidor pelo convite. O código é o que o dono mandou, não o do servidor.
    pub async fn join_invite(&self, code: &str) -> Result<ServerSummary, HttpError> {
        self.post(&format!("/api/invites/{code}"), &serde_json::json!({})).await
    }

    /// Sorteia um convite novo. O anterior para de valer na hora.
    pub async fn regenerate_invite(&self, server: i64) -> Result<String, HttpError> {
        let answer: Value = self.post(&format!("/api/servers/{server}/invite"), &serde_json::json!({})).await?;

        Ok(answer["invite_code"].as_str().unwrap_or_default().to_owned())
    }

    pub async fn leave_server(&self, server: i64) -> Result<(), HttpError> {
        let _: Value = self.post(&format!("/api/servers/{server}/leave"), &serde_json::json!({})).await?;

        Ok(())
    }

    /// Abre um canal no servidor. `kind` é o que separa texto de voz, e o servidor recusa
    /// qualquer outra coisa.
    ///
    /// Não devolve o canal: a resposta do `store` vem sem a permissão calculada, e quem
    /// chama precisa da árvore inteira de qualquer jeito para desenhar a coluna de novo.
    pub async fn create_channel(&self, server: i64, name: &str, kind: ChannelKind) -> Result<(), HttpError> {
        let _: Value = self
            .post(
                &format!("/api/servers/{server}/channels"),
                &serde_json::json!({ "name": name, "type": kind }),
            )
            .await?;

        Ok(())
    }

    pub async fn friends(&self) -> Result<Vec<Friendship>, HttpError> {
        self.get("/api/friends").await
    }

    /// Pede amizade pelo e-mail. O servidor é quem diz se a pessoa existe.
    pub async fn add_friend(&self, email: &str) -> Result<Friendship, HttpError> {
        self.post("/api/friends", &serde_json::json!({ "email": email })).await
    }

    /// Responde a um pedido. Aceitar é uma ação nomeada (`accept`); recusar é apagar, porque
    /// o servidor não guarda "não" — só `accept` e `block` passam pelo `PATCH`.
    pub async fn answer_friend(&self, friendship: i64, accept: bool) -> Result<(), HttpError> {
        if accept {
            let _: Value = self
                .send(
                    self.http
                        .patch(self.url(&format!("/api/friends/{friendship}")))
                        .json(&serde_json::json!({ "action": "accept" })),
                    "/api/friends",
                )
                .await?;

            return Ok(());
        }

        let _: Value = self
            .send(self.http.delete(self.url(&format!("/api/friends/{friendship}"))), "/api/friends")
            .await?;

        Ok(())
    }

    /// As conversas abertas, a última frase de cada uma e quantas faltam ler.
    pub async fn conversations(&self) -> Result<Vec<Conversation>, HttpError> {
        self.get("/api/dm").await
    }

    pub async fn direct_messages(&self, user: i64) -> Result<Vec<DirectMessage>, HttpError> {
        self.get(&format!("/api/dm/{user}")).await
    }

    pub async fn send_direct(&self, user: i64, body: &str) -> Result<DirectMessage, HttpError> {
        self.post(&format!("/api/dm/{user}"), &serde_json::json!({ "body": body })).await
    }

    /// Marca a conversa como lida. Sem isto o contador de não lidas nunca zera.
    pub async fn read_conversation(&self, user: i64) -> Result<(), HttpError> {
        let _: Value = self.post(&format!("/api/dm/{user}/read"), &serde_json::json!({})).await?;

        Ok(())
    }

    pub async fn messages(&self, channel: &str) -> Result<Vec<Message>, HttpError> {
        self.get(&format!("/api/channels/{channel}/messages")).await
    }

    pub async fn send_message(&self, channel: &str, body: &str) -> Result<Message, HttpError> {
        self.post(
            &format!("/api/channels/{channel}/messages"),
            &serde_json::json!({ "body": body }),
        )
        .await
    }

    pub async fn edit_message(&self, message: i64, body: &str) -> Result<Message, HttpError> {
        let path = format!("/api/messages/{message}");

        self.send(
            self.http
                .patch(self.url(&path))
                .json(&serde_json::json!({ "body": body })),
            &path,
        )
        .await
    }

    pub async fn delete_message(&self, message: i64) -> Result<(), HttpError> {
        let path = format!("/api/messages/{message}");

        self.send::<Value>(self.http.delete(self.url(&path)), &path)
            .await
            .map(|_| ())
    }

    /// A versão publicada mais nova do que esta, com o endereço do instalador desta
    /// plataforma (`darwin-aarch64`, `windows-x86_64`…). `None` quando não há nada mais novo —
    /// inclusive quando nada foi publicado ainda, que é o 404 do `latest.json`.
    pub async fn newer_release(&self, platform: &str) -> Option<(String, String)> {
        let manifest: Value = self
            .http
            .get(self.url("/downloads/latest.json"))
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        let version = manifest["version"].as_str()?;
        let url = manifest["platforms"][platform]["url"].as_str()?;

        is_newer(version, env!("CARGO_PKG_VERSION")).then(|| (version.to_owned(), url.to_owned()))
    }

    /// Uma rota do mapa de `routes.rs`, pelo nome. É por onde passa tudo o que não tem
    /// método próprio aqui: escrever no servidor, amigos, mensagens diretas.
    pub async fn perform(
        &self,
        name: &str,
        params: &Value,
        body: &Value,
    ) -> Result<Value, HttpError> {
        let Some((method, path)) = crate::routes::resolve(name, params) else {
            tracing::warn!(name, "rota desconhecida ou parâmetro faltando");

            return Err(Failure::Invalid.into());
        };

        let request = self.http.request(method.clone(), self.url(&path));
        let request = if method == reqwest::Method::GET || body.is_null() {
            request
        } else {
            request.json(body)
        };

        self.send(request, &path).await
    }

    /// Manda um arquivo por uma rota do mapa (foto, ícone do servidor, imagem de mensagem).
    /// `fields` são os outros campos do formulário, como o texto que acompanha a imagem.
    pub async fn upload(
        &self,
        name: &str,
        params: &Value,
        field: &str,
        files: &[std::path::PathBuf],
        fields: &Value,
    ) -> Result<Value, HttpError> {
        let Some((method, path)) = crate::routes::resolve(name, params) else {
            return Err(Failure::Invalid.into());
        };

        let mut form = reqwest::multipart::Form::new();

        for (key, value) in fields.as_object().into_iter().flatten() {
            if let Some(text) = value.as_str() {
                form = form.text(key.clone(), text.to_owned());
            }
        }

        for file in files {
            let bytes = tokio::fs::read(file).await.map_err(|failure| {
                tracing::warn!(%failure, "o arquivo escolhido não abriu");

                HttpError::Failed(Failure::Invalid)
            })?;

            let name = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mime = mime_of(&name).unwrap_or("application/octet-stream");
            let part = reqwest::multipart::Part::bytes(bytes)
                .file_name(name)
                .mime_str(mime)
                .map_err(|_| HttpError::Failed(Failure::Invalid))?;

            form = form.part(field.to_owned(), part);
        }

        self.send(
            self.http.request(method, self.url(&path)).multipart(form),
            &path,
        )
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
        self.token_from(&format!("/api/channels/{channel}/voice/token"))
            .await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, HttpError> {
        self.send(self.http.get(self.url(path)), path).await
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T, HttpError> {
        self.send(self.http.post(self.url(path)).json(body), path)
            .await
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
            Some(object) if object.len() == 1 && object.contains_key("data") => {
                body["data"].clone()
            }
            _ => body,
        };

        serde_json::from_value(payload).map_err(|failure| {
            tracing::warn!(%failure, path, "a resposta não tem o formato esperado");

            Failure::ServerBroke.into()
        })
    }
}

/// `1.10.0` é mais novo que `1.9.3`: compara número a número, e não letra a letra.
fn is_newer(candidate: &str, current: &str) -> bool {
    let numbers = |version: &str| {
        version
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };

    numbers(candidate) > numbers(current)
}

/// O tipo de uma imagem pelo nome do arquivo. O Laravel valida pelo conteúdo; isto só evita
/// que a parte suba como `application/octet-stream`.
fn mime_of(name: &str) -> Option<&'static str> {
    match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
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
    body["errors"]
        .as_object()?
        .iter()
        .find_map(|(field, messages)| {
            let message = messages.as_array()?.first()?.as_str()?;

            Some((field.clone(), message.to_owned()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_and_not_by_letter() {
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(is_newer("0.0.41", "0.0.40"));
        assert!(!is_newer("0.0.40", "0.0.40"));
        assert!(!is_newer("0.9.9", "1.0.0"));
    }

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

        assert_eq!(
            refusal(422, &body, "/api/x"),
            HttpError::Failed(Failure::Invalid)
        );
    }

    #[test]
    fn the_base_address_never_ends_up_with_two_slashes() {
        assert_eq!(
            Api::new("http://127.0.0.1:8000/")
                .expect("build")
                .url("/api/config"),
            "http://127.0.0.1:8000/api/config"
        );
        assert_eq!(
            Api::new("http://127.0.0.1:8000")
                .expect("build")
                .url("/api/config"),
            "http://127.0.0.1:8000/api/config"
        );
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
