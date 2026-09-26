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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::failure::Failure;
use crate::models::{
    AuthToken, ChannelKind, Config, Conversation, DirectMessage, Friendship, Message,
    ServerSummary, ServerTree, User,
};

/// Dez segundos, o mesmo do app de hoje. Passar disto a pessoa já desistiu e clicou de novo.
const TIMEOUT: Duration = Duration::from_secs(10);

/// O instalador inteiro numa conexão lenta passa dos dez segundos, e ali ninguém clica de novo.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

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

/// O que acontece com a sessão por conta própria, fora de um login: o par foi trocado (e tem
/// de ir para o disco), ou acabou (e a interface volta ao login).
pub enum Renewal<'tokens> {
    Renewed { token: &'tokens str, refresh: &'tokens str },
    Ended,
}

type SessionHook = Arc<dyn Fn(Renewal<'_>) + Send + Sync>;

impl From<Failure> for HttpError {
    fn from(failure: Failure) -> Self {
        Self::Failed(failure)
    }
}

pub struct Api {
    base: String,
    token: Mutex<Option<String>>,
    refresh: Mutex<Option<String>>,
    /// Uma renovação por vez: duas em paralelo gastariam o mesmo token de renovação, e a
    /// segunda derrubaria a sessão que a primeira acabou de salvar.
    renewing: tokio::sync::Mutex<()>,
    session: Mutex<Option<SessionHook>>,
    /// A última árvore vista de cada servidor. Trocar de servidor desenha os canais daqui,
    /// na hora, e a resposta fresca chega por cima: o que espera a rede são só as mensagens.
    trees: Mutex<HashMap<i64, ServerTree>>,
    http: reqwest::Client,
}

impl Api {
    pub fn new(base: &str) -> anyhow::Result<Self> {
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            token: Mutex::new(None),
            refresh: Mutex::new(None),
            renewing: tokio::sync::Mutex::new(()),
            session: Mutex::new(None),
            trees: Mutex::new(HashMap::new()),
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
        let tree: ServerTree = self.get(&format!("/api/servers/{server}")).await?;

        self.trees().insert(server, tree.clone());

        Ok(tree)
    }

    /// A árvore que já se conhece, sem ir à rede. Pode estar velha: quem a desenha pede a
    /// fresca em seguida com `tree`.
    pub fn known_tree(&self, server: i64) -> Option<ServerTree> {
        self.trees().get(&server).cloned()
    }

    /// Busca a árvore de cada servidor logo depois da lista, para que o primeiro clique em
    /// qualquer um já encontre os canais. Falha aqui não é de ninguém: o clique busca de novo.
    pub async fn warm_trees(&self, servers: &[i64]) {
        // ponytail: uma de cada vez; com dezenas de servidores vale buscar em paralelo com um teto.
        for server in servers {
            if let Err(failure) = self.tree(*server).await {
                tracing::debug!(server, ?failure, "a árvore não veio no aquecimento");
            }
        }
    }

    fn trees(&self) -> std::sync::MutexGuard<'_, HashMap<i64, ServerTree>> {
        self.trees
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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

        // Outra conta, outras permissões: a árvore de quem saiu não serve a quem entrou.
        self.trees().clear();
    }

    pub fn token(&self) -> Option<String> {
        self.token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn set_refresh(&self, refresh: Option<String>) {
        *self.refresh.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = refresh;
    }

    pub fn refresh_token(&self) -> Option<String> {
        self.refresh.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// Quem guarda a sessão no disco e leva a tela ao login quando ela acaba. Ver
    /// `App::keep_session`.
    pub fn on_session(&self, hook: impl Fn(Renewal<'_>) + Send + Sync + 'static) {
        *self.session.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(hook));
    }

    /// Sai da conta: esquece os tokens agora — a tela não espera a rede — e devolve o pedido
    /// que os derruba no servidor, para rodar em segundo plano. Sem rede, o de acesso vence
    /// sozinho num dia e o de renovação em sessenta.
    pub fn sign_out(&self) -> impl Future<Output = ()> + Send + 'static {
        let (token, refresh) = (self.token(), self.refresh_token());
        let (http, url) = (self.http.clone(), self.url("/api/auth/logout"));

        self.set_token(None);
        self.set_refresh(None);

        async move {
            let Some(token) = token else {
                return;
            };

            let sent = http
                .post(url)
                .bearer_auth(token)
                .header("accept", "application/json")
                .json(&serde_json::json!({ "refresh_token": refresh }))
                .send()
                .await;

            if let Err(failure) = sent {
                tracing::info!(%failure, "sair: o servidor não soube, e os tokens vencem sozinhos");
            }
        }
    }

    /// Troca o par com o token de renovação. `stale` é o token que levou o 401: se ele já não
    /// é o atual, outra chamada renovou primeiro, e basta tentar de novo.
    async fn renew(&self, stale: &str) -> bool {
        let _turn = self.renewing.lock().await;

        if self.token().as_deref() != Some(stale) {
            return self.token().is_some();
        }

        let Some(refresh) = self.refresh_token() else {
            return false;
        };

        let answer = self
            .http
            .post(self.url("/api/auth/refresh"))
            .header("accept", "application/json")
            .json(&serde_json::json!({ "refresh_token": refresh }))
            .send()
            .await;
        let renewed = match answer {
            Ok(answer) if answer.status().is_success() => answer.json::<Value>().await.ok(),
            Ok(answer) => {
                tracing::info!(status = answer.status().as_u16(), "a renovação foi recusada");

                None
            }
            Err(failure) => {
                tracing::warn!(%failure, "a renovação não chegou ao servidor");

                // Sem rede não é sessão acabada: o par continua valendo para quando ela voltar.
                return false;
            }
        };
        let pair = renewed.and_then(|body| serde_json::from_value::<AuthToken>(body["data"].clone()).ok());

        let Some(AuthToken { token, refresh_token: Some(refresh), .. }) = pair else {
            self.end(stale);

            return false;
        };

        self.set_token(Some(token.clone()));
        self.set_refresh(Some(refresh.clone()));
        self.tell(Renewal::Renewed { token: &token, refresh: &refresh });

        true
    }

    /// A sessão acabou de vez: sai da memória, e a interface volta ao login. Só se o token
    /// que falhou ainda é o atual — quem já saiu ou entrou de novo não é derrubado.
    fn end(&self, stale: &str) {
        if self.token().as_deref() != Some(stale) {
            return;
        }

        self.set_token(None);
        self.set_refresh(None);
        self.tell(Renewal::Ended);
    }

    fn tell(&self, renewal: Renewal<'_>) {
        let hook = self.session.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();

        if let Some(hook) = hook {
            hook(renewal);
        }
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
                &serde_json::json!({ "email": email, "password": password, "device": device, "refresh": true }),
            )
            .await?;

        self.adopt(&answer.token, answer.refresh_token.as_deref());

        Ok(answer)
    }

    /// Um login novo, por senha ou pelo Google: o par vai para a memória e para o disco.
    pub fn adopt(&self, token: &str, refresh: Option<&str>) {
        self.set_token(Some(token.to_owned()));
        self.set_refresh(refresh.map(str::to_owned));

        if let Some(refresh) = refresh {
            self.tell(Renewal::Renewed { token, refresh });
        }
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
        self.post("/api/servers", &serde_json::json!({ "name": name }))
            .await
    }

    /// Entra num servidor pelo convite. O código é o que o dono mandou, não o do servidor.
    pub async fn join_invite(&self, code: &str) -> Result<ServerSummary, HttpError> {
        self.post(&format!("/api/invites/{code}"), &serde_json::json!({}))
            .await
    }

    /// Sorteia um convite novo. O anterior para de valer na hora.
    pub async fn regenerate_invite(&self, server: i64) -> Result<String, HttpError> {
        let answer: Value = self
            .post(
                &format!("/api/servers/{server}/invite"),
                &serde_json::json!({}),
            )
            .await?;

        Ok(answer["invite_code"]
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    pub async fn leave_server(&self, server: i64) -> Result<(), HttpError> {
        let _: Value = self
            .post(
                &format!("/api/servers/{server}/leave"),
                &serde_json::json!({}),
            )
            .await?;

        Ok(())
    }

    /// Abre um canal no servidor. `kind` é o que separa texto de voz, e o servidor recusa
    /// qualquer outra coisa.
    ///
    /// Não devolve o canal: a resposta do `store` vem sem a permissão calculada, e quem
    /// chama precisa da árvore inteira de qualquer jeito para desenhar a coluna de novo.
    pub async fn create_channel(
        &self,
        server: i64,
        name: &str,
        kind: ChannelKind,
    ) -> Result<(), HttpError> {
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
        self.post("/api/friends", &serde_json::json!({ "email": email }))
            .await
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
            .send(
                self.http
                    .delete(self.url(&format!("/api/friends/{friendship}"))),
                "/api/friends",
            )
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
        self.post(
            &format!("/api/dm/{user}"),
            &serde_json::json!({ "body": body }),
        )
        .await
    }

    /// Marca a conversa como lida. Sem isto o contador de não lidas nunca zera.
    pub async fn read_conversation(&self, user: i64) -> Result<(), HttpError> {
        let _: Value = self
            .post(&format!("/api/dm/{user}/read"), &serde_json::json!({}))
            .await?;

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

    /// A versão publicada mais nova do que esta, com o instalador desta plataforma
    /// (`darwin-aarch64`, `windows-x86_64-nsis`…) e a assinatura dele. `None` quando não há
    /// nada mais novo — inclusive quando nada foi publicado ainda, que é o 404 do `latest.json`.
    pub async fn newer_release(&self, platform: &str) -> Option<crate::update::Release> {
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
        let installer = &manifest["platforms"][platform];

        crate::update::is_newer(version, env!("CARGO_PKG_VERSION")).then(|| crate::update::Release {
            version: version.to_owned(),
            url: installer["url"].as_str().unwrap_or_default().to_owned(),
            signature: installer["signature"].as_str().unwrap_or_default().to_owned(),
        })
        .filter(|release| !release.url.is_empty())
    }

    /// Um arquivo inteiro, com o caminho contado: `progress` recebe o baixado e o total, quando
    /// o servidor o diz.
    pub async fn download(
        &self,
        url: &str,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Option<Vec<u8>> {
        let mut answer = self
            .http
            .get(url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let total = answer.content_length();
        let mut bytes = Vec::new();

        while let Some(chunk) = answer.chunk().await.ok()? {
            bytes.extend_from_slice(&chunk);
            progress(bytes.len() as u64, total);
        }

        Some(bytes)
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

    /// Manda com o token da sessão. Um 401 com sessão aberta renova o par e repete o pedido
    /// uma vez; sem como renovar, a sessão acaba e a interface volta ao login.
    async fn send<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
        path: &str,
    ) -> Result<T, HttpError> {
        let again = request.try_clone();
        let sent = self.token();
        let answer = self.exchange(request, sent.as_deref(), path).await;

        let answer = match (answer, sent) {
            (Err(HttpError::Failed(Failure::SignedOut)), Some(stale)) if !path.starts_with("/api/auth/") => {
                if !self.renew(&stale).await {
                    return Err(Failure::SignedOut.into());
                }

                // Envio de arquivo não se repete (o corpo já foi); a próxima tentativa já sai
                // com o token novo.
                let Some(again) = again else {
                    return Err(Failure::Unreachable.into());
                };

                self.exchange(again, self.token().as_deref(), path).await
            }
            (answer, _) => answer,
        }?;

        serde_json::from_value(answer).map_err(|failure| {
            tracing::warn!(%failure, path, "a resposta não tem o formato esperado");

            Failure::ServerBroke.into()
        })
    }

    async fn exchange(
        &self,
        request: reqwest::RequestBuilder,
        token: Option<&str>,
        path: &str,
    ) -> Result<Value, HttpError> {
        let request = request.header("accept", "application/json");

        let request = match token {
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

        Ok(match body.as_object() {
            Some(object) if object.len() == 1 && object.contains_key("data") => {
                body["data"].clone()
            }
            _ => body,
        })
    }
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

    // No login o 401 é senha errada, e não sessão vencida: a frase do Laravel ("E-mail ou
    // senha não conferem.") vai para o campo do e-mail.
    if status == 401
        && (path == "/api/auth/login" || path == "/api/auth/register")
        && let Some(message) = body["message"].as_str()
    {
        return HttpError::Invalid { field: "email".to_owned(), message: message.to_owned() };
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

    /// Um Laravel de mentira para a renovação: o token `a2` vale, qualquer outro é 401; a
    /// primeira renovação entrega o par `a2`/`r2`, e as seguintes são recusadas.
    async fn serve_renewals() -> String {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("porta");
        let base = format!("http://{}", listener.local_addr().expect("endereço"));
        let renewals = Arc::new(AtomicUsize::new(0));

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = vec![0; 8192];
                let size = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..size]).to_string();
                let (status, body) = if request.starts_with("POST /api/auth/refresh") {
                    if renewals.fetch_add(1, Ordering::SeqCst) == 0 {
                        (200, serde_json::json!({ "data": { "token": "a2", "refresh_token": "r2", "user": null } }))
                    } else {
                        (401, serde_json::json!({ "message": "Sua sessão terminou. Entre de novo." }))
                    }
                } else if request.to_lowercase().contains("authorization: bearer a2") {
                    (200, serde_json::json!({ "data": { "id": 1, "name": "Ana", "email": "ana@local.test", "avatar_url": null } }))
                } else {
                    (401, serde_json::json!({ "message": "Unauthenticated." }))
                };
                let body = body.to_string();
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );

                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });

        base
    }

    #[tokio::test]
    async fn an_expired_token_is_renewed_once_and_the_call_goes_through() {
        let api = Api::new(&serve_renewals().await).expect("api");
        let told = Arc::new(Mutex::new(Vec::new()));

        api.set_token(Some("a1".into()));
        api.set_refresh(Some("r1".into()));
        api.on_session({
            let told = told.clone();

            move |renewal| {
                told.lock().expect("lista").push(match renewal {
                    Renewal::Renewed { token, refresh } => format!("{token}/{refresh}"),
                    Renewal::Ended => "acabou".to_owned(),
                });
            }
        });

        let user = api.me().await.expect("renovou e repetiu o pedido");

        assert_eq!(user.name, "Ana");
        assert_eq!((api.token().as_deref(), api.refresh_token().as_deref()), (Some("a2"), Some("r2")));
        assert_eq!(*told.lock().expect("lista"), ["a2/r2"]);

        api.set_token(Some("a3".into()));

        assert_eq!(api.me().await.err(), Some(HttpError::Failed(Failure::SignedOut)));
        assert_eq!((api.token(), api.refresh_token()), (None, None), "a sessão que não renova sai da memória");
        assert_eq!(told.lock().expect("lista").last().map(String::as_str), Some("acabou"));
    }

    #[tokio::test]
    async fn a_wrong_password_says_so_instead_of_ending_a_session() {
        let api = Api::new(&serve_renewals().await).expect("api");
        let refused = api.login("ana@local.test", "errada", "teste").await;

        assert!(
            matches!(&refused, Err(HttpError::Invalid { field, .. }) if field == "email"),
            "saiu {refused:?}"
        );
    }

    /// Um Laravel de mentira que responde a mesma árvore a quantos pedidos vierem.
    async fn serve_a_tree() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let body = json!({
                    "id": 7, "name": "Estúdio", "owner_id": 1, "invite_code": null, "icon_url": null,
                    "me": { "user_id": 1, "permissions": 0, "top_position": 0 },
                    "roles": [], "channels": [], "members": [],
                })
                .to_string();
                let mut request = [0_u8; 2048];
                let _ = socket.read(&mut request).await;
                let answer = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(answer.as_bytes()).await;
            }
        });

        format!("http://{address}")
    }

    #[tokio::test]
    async fn a_seen_tree_is_kept_for_the_next_click_and_forgotten_with_the_account() {
        let api = Api::new(&serve_a_tree().await).expect("api");

        assert!(api.known_tree(7).is_none(), "nada visto ainda");

        api.warm_trees(&[7]).await;

        assert_eq!(
            api.known_tree(7).map(|tree| tree.name),
            Some("Estúdio".to_owned())
        );

        api.set_token(None);

        assert!(
            api.known_tree(7).is_none(),
            "a árvore de quem saiu não fica para quem entra"
        );
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
