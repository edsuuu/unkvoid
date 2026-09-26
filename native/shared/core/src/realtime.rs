//! O tempo real do chat e da presença: o mesmo socket do SFU, apresentado com o token que o
//! Laravel assina, e os canais que a interface assina (`user.{id}`, `server.{id}`,
//! `channel.{id}`). É o `Realtime.ts` do React.
//!
//! O que chega vai para a fila da interface no formato de sempre — `{event, channel, data}` —,
//! e mais quatro avisos daqui: `presence.here` com quem já estava num canal de presença ao
//! assinar, `realtime.lost` quando o socket cai, `realtime.back` quando ele volta com tudo
//! reassinado (hora de reler o que pode ter passado) e `realtime.closed` quando desiste.

use std::collections::{BTreeSet, HashSet};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use anyhow::Result;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::api::Api;
use crate::chimes::Chime;
use crate::client::SfuClient;
use crate::protocol::Event;
use crate::reconnect::Backoff;

pub struct Realtime {
    url: String,
    api: Arc<Api>,
    client: Mutex<Option<Arc<SfuClient>>>,
    /// O que está assinado, para reassinar quando o socket volta.
    channels: Mutex<BTreeSet<String>>,
    updates: Sender<String>,
}

impl Realtime {
    pub async fn connect(url: &str, api: Arc<Api>, updates: Sender<String>) -> Result<Arc<Self>> {
        let (client, events) = SfuClient::connect(url).await?;
        let realtime = Arc::new(Self {
            url: url.to_owned(),
            api,
            client: Mutex::new(Some(Arc::clone(&client))),
            channels: Mutex::default(),
            updates,
        });

        realtime.identify(&client).await?;
        tokio::spawn(relay(Arc::downgrade(&realtime), events));

        Ok(realtime)
    }

    /// Assina um canal. Canal de presença responde com quem já está lá, que sai como
    /// `presence.here` antes de qualquer `presence.joining`.
    pub async fn subscribe(&self, channel: &str) -> Result<()> {
        lock(&self.channels).insert(channel.to_owned());

        let Some(client) = self.client() else {
            return Ok(());
        };

        self.join(&client, channel).await
    }

    pub async fn unsubscribe(&self, channel: &str) {
        if !lock(&self.channels).remove(channel) {
            return;
        }

        if let Some(client) = self.client() {
            let _ = client.unsubscribe(channel).await;
        }
    }

    /// Fecha o socket: a conta saiu, e o que chegar para ela não é mais de ninguém aqui.
    pub fn close(&self) {
        if let Some(client) = lock(&self.client).take() {
            client.close();
        }
    }

    async fn identify(&self, client: &SfuClient) -> Result<()> {
        let token = self
            .api
            .realtime_token()
            .await
            .map_err(|failure| anyhow::anyhow!("o token do tempo real não veio: {failure:?}"))?;

        client.identify(&token).await.map(|_| ())
    }

    async fn join(&self, client: &SfuClient, channel: &str) -> Result<()> {
        let answer = client.subscribe(channel).await?;

        if let Some(members) = answer.get("members") {
            self.tell("presence.here", Some(channel), json!({ "members": members }));
        }

        Ok(())
    }

    fn client(&self) -> Option<Arc<SfuClient>> {
        lock(&self.client).clone()
    }

    fn tell(&self, event: &str, channel: Option<&str>, data: Value) -> bool {
        self.updates
            .send(json!({ "event": event, "channel": channel, "data": data }).to_string())
            .is_ok()
    }
}

/// Repassa o que chega e, quando o socket cai, o traz de volta com a mesma conta e os mesmos
/// canais. Termina quando o `Realtime` some ou a interface para de ouvir.
async fn relay(realtime: Weak<Realtime>, mut events: UnboundedReceiver<Event>) {
    loop {
        while let Some(event) = events.recv().await {
            let Some(realtime) = realtime.upgrade() else {
                return;
            };

            if !realtime.tell(&event.name, event.channel.as_deref(), event.data) {
                return;
            }
        }

        let Some(held) = realtime.upgrade() else {
            return;
        };

        // Fechado de propósito não volta.
        if held.client().is_none() || !held.tell("realtime.lost", None, json!({})) {
            return;
        }

        let mut backoff = Backoff::default();

        events = loop {
            let Some(wait) = backoff.next_delay() else {
                held.tell("realtime.closed", None, json!({}));

                return;
            };

            tokio::time::sleep(wait).await;

            let Ok((client, fresh)) = SfuClient::connect(&held.url).await else {
                continue;
            };

            if held.identify(&client).await.is_err() {
                continue;
            }

            *lock(&held.client) = Some(Arc::clone(&client));

            let channels: Vec<String> = lock(&held.channels).iter().cloned().collect();

            for channel in channels {
                if let Err(failure) = held.join(&client, &channel).await {
                    tracing::warn!(%failure, channel, "tempo real: o canal não voltou");
                }
            }

            break fresh;
        };

        held.tell("realtime.back", None, json!({}));
    }
}

/// O que um evento do tempo real pede da interface — o mesmo que o React e o Mac fazem com
/// ele. A interface só executa: relê o que mudou, toca, e mostra o aviso.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Reading {
    /// As mensagens do canal com este id mudaram: relê se é o aberto, no texto ou na voz.
    pub messages_of: Option<String>,
    /// Chegou mensagem de outra pessoa nesse canal: conta como não lida onde não se está vendo.
    pub unread: bool,
    /// Uma conversa direta mudou. `direct_with` é com quem, quando o evento diz — o apagar só
    /// manda o id da mensagem, e aí vale a conversa que estiver aberta.
    pub direct: bool,
    pub direct_with: Option<i64>,
    pub friends: bool,
    /// A árvore do servidor aberto mudou (nome, canal, cargo, alguém entrou na voz): relê.
    pub tree: bool,
    /// A pessoa foi tirada deste servidor: a lista muda, e se era o aberto, volta à Home.
    pub removed_from: Option<i64>,
    /// Quem está online no servidor aberto mudou.
    pub presence: Option<Presence>,
    /// O socket voltou depois de cair: relê tudo o que pode ter passado.
    pub catch_up: bool,
    pub chime: Option<Chime>,
    pub notice: Option<Notice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presence {
    Here(Vec<i64>),
    Joining(i64),
    Leaving(i64),
}

impl Presence {
    pub fn apply(&self, online: &mut HashSet<i64>) {
        match self {
            Self::Here(everyone) => *online = everyone.iter().copied().collect(),
            Self::Joining(person) => {
                online.insert(*person);
            }
            Self::Leaving(person) => {
                online.remove(person);
            }
        }
    }
}

/// Um aviso curto no pé da janela. `error` pinta de vermelho e fica mais tempo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub error: bool,
}

impl Notice {
    pub fn info(text: impl Into<String>) -> Self {
        Self { text: text.into(), error: false }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { text: text.into(), error: true }
    }
}

/// Lê um evento da fila. `me` é a conta; `talking` é com quem a conversa direta está aberta.
pub fn read(event: &str, channel: Option<&str>, data: &Value, me: i64, talking: Option<i64>) -> Reading {
    let channel_id = channel
        .and_then(|channel| channel.strip_prefix("channel."))
        .map(str::to_owned);
    let mut reading = Reading::default();

    match event {
        "MessageSent" => {
            let mine = data["message"]["user"]["id"].as_i64() == Some(me);

            reading.messages_of = channel_id;
            reading.unread = !mine;
            reading.chime = (!mine).then_some(Chime::Message);
        }
        "MessageUpdated" | "MessageDeleted" => reading.messages_of = channel_id,
        "DirectMessageCreated" | "DirectMessageUpdated" => {
            let sender = data["message"]["sender"]["id"].as_i64();
            let mine = sender == Some(me);
            let person = if mine { data["recipient"]["id"].as_i64() } else { sender };

            reading.direct = true;
            reading.direct_with = person;

            if event == "DirectMessageCreated" && !mine {
                reading.chime = Some(Chime::Message);

                if person != talking {
                    let name = data["message"]["sender"]["name"].as_str().unwrap_or_default();
                    let body: String = data["message"]["body"].as_str().unwrap_or_default().chars().take(60).collect();

                    reading.notice = Some(Notice::info(format!("{name}: {body}")));
                }
            }
        }
        "DirectMessageDeleted" => reading.direct = true,
        "FriendshipUpdated" => {
            let friendship = &data["friendship"];

            reading.friends = true;

            if data["removed"].as_bool() != Some(true)
                && friendship["status"] == "pending"
                && friendship["addressee"]["id"].as_i64() == Some(me)
            {
                let name = friendship["requester"]["name"].as_str().unwrap_or_default();

                reading.notice = Some(Notice::info(format!("{name} quer ser seu amigo")));
            }
        }
        "MemberRemoved" => {
            reading.removed_from = data["server_id"].as_i64();
            reading.notice = Some(Notice::error(if data["reason"] == "banned" {
                "você foi banido deste servidor"
            } else {
                "você foi expulso deste servidor"
            }));
        }
        "ServerUpdated" | "VoiceStateUpdated" => reading.tree = true,
        "presence.here" => {
            let everyone = data["members"]
                .as_array()
                .map(|members| members.iter().filter_map(|member| member["id"].as_str().and_then(person_id)).collect())
                .unwrap_or_default();

            reading.presence = Some(Presence::Here(everyone));
        }
        "presence.joining" => reading.presence = data["id"].as_str().and_then(person_id).map(Presence::Joining),
        "presence.leaving" => reading.presence = data["id"].as_str().and_then(person_id).map(Presence::Leaving),
        "realtime.back" => reading.catch_up = true,
        "realtime.closed" => {
            reading.notice = Some(Notice::error(
                "o tempo real desistiu de voltar: reabra o app para o chat e a presença voltarem",
            ));
        }
        _ => {}
    }

    reading
}

/// A presença manda a pessoa como `user:5`; o resto do app a conhece por `5`.
pub fn person_id(identity: &str) -> Option<i64> {
    identity.rsplit(':').next()?.parse().ok()
}

fn lock<T>(cell: &Mutex<T>) -> MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_from_someone_else_chimes_and_counts_as_unread() {
        let theirs = json!({ "message": { "user": { "id": 2 } } });
        let mine = json!({ "message": { "user": { "id": 1 } } });

        let reading = read("MessageSent", Some("channel.abc"), &theirs, 1, None);

        assert_eq!(reading.messages_of.as_deref(), Some("abc"));
        assert!(reading.unread);
        assert_eq!(reading.chime, Some(Chime::Message));

        let reading = read("MessageSent", Some("channel.abc"), &mine, 1, None);

        assert!(!reading.unread);
        assert_eq!(reading.chime, None);
    }

    #[test]
    fn a_direct_message_warns_only_when_its_conversation_is_not_open() {
        let data = json!({
            "message": { "body": "oi, tudo bem?", "sender": { "id": 2, "name": "Ana" } },
            "recipient": { "id": 1, "name": "Eu" },
        });

        let closed = read("DirectMessageCreated", Some("user.1"), &data, 1, None);

        assert_eq!(closed.direct_with, Some(2));
        assert_eq!(closed.chime, Some(Chime::Message));
        assert_eq!(closed.notice, Some(Notice::info("Ana: oi, tudo bem?")));

        let open = read("DirectMessageCreated", Some("user.1"), &data, 1, Some(2));

        assert_eq!(open.chime, Some(Chime::Message));
        assert_eq!(open.notice, None);
    }

    #[test]
    fn my_own_direct_message_points_at_who_received_it_and_stays_quiet() {
        let data = json!({
            "message": { "body": "oi", "sender": { "id": 1, "name": "Eu" } },
            "recipient": { "id": 2, "name": "Ana" },
        });
        let reading = read("DirectMessageCreated", Some("user.1"), &data, 1, None);

        assert_eq!(reading.direct_with, Some(2));
        assert_eq!((reading.chime, reading.notice), (None, None));
    }

    #[test]
    fn a_friend_request_to_me_is_announced() {
        let data = json!({ "friendship": { "status": "pending", "requester": { "id": 2, "name": "Ana" }, "addressee": { "id": 1 } } });

        assert_eq!(read("FriendshipUpdated", None, &data, 1, None).notice, Some(Notice::info("Ana quer ser seu amigo")));
        assert_eq!(read("FriendshipUpdated", None, &data, 2, None).notice, None);
    }

    #[test]
    fn being_removed_from_a_server_says_how() {
        let banned = read("MemberRemoved", None, &json!({ "server_id": 7, "reason": "banned" }), 1, None);

        assert_eq!(banned.removed_from, Some(7));
        assert_eq!(banned.notice, Some(Notice::error("você foi banido deste servidor")));
        assert_eq!(
            read("MemberRemoved", None, &json!({ "server_id": 7, "reason": "kicked" }), 1, None).notice,
            Some(Notice::error("você foi expulso deste servidor"))
        );
    }

    #[test]
    fn presence_keeps_who_is_online() {
        let mut online = HashSet::new();
        let here = json!({ "members": [{ "id": "user:1" }, { "id": "user:2" }] });

        for (event, data) in [
            ("presence.here", here),
            ("presence.joining", json!({ "id": "user:3" })),
            ("presence.leaving", json!({ "id": "user:1" })),
        ] {
            read(event, Some("server.9"), &data, 1, None).presence.expect("presença").apply(&mut online);
        }

        assert_eq!(online, HashSet::from([2, 3]));
    }

    /// Contra a pilha local no ar: a conta A entra no tempo real e assina o canal de texto e o
    /// dela; a conta B escreve no canal e manda uma mensagem direta, e os dois eventos têm de
    /// chegar — já lidos pelo `read` como os apps os leem.
    ///
    /// `UNKVOID_A=email UNKVOID_B=email UNKVOID_PASSWORD=… UNKVOID_CHANNEL=<ulid>
    ///  cargo test -p core-app live_events -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_events_reach_the_account_that_follows_them() {
        let variable = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("falta {name}"));
        let server = std::env::var("UNKVOID_SERVER").unwrap_or_else(|_| "http://127.0.0.1:8000".into());
        let (ana, bia) = (Arc::new(Api::new(&server).expect("api")), Api::new(&server).expect("api"));
        let password = variable("UNKVOID_PASSWORD");
        let me = ana.login(&variable("UNKVOID_A"), &password, "teste").await.expect("A entrou").user.expect("A").id;
        let other = bia.login(&variable("UNKVOID_B"), &password, "teste").await.expect("B entrou").user.expect("B");
        let sfu = ana.config().await.expect("config").sfu;
        let channel = variable("UNKVOID_CHANNEL");
        let (updates, heard) = std::sync::mpsc::channel();
        let live = Realtime::connect(&sfu, ana.clone(), updates).await.expect("tempo real");

        live.subscribe(&format!("user.{me}")).await.expect("canal da conta");
        live.subscribe(&format!("channel.{channel}")).await.expect("canal de texto");

        bia.send_message(&channel, "oi do teste vivo").await.expect("B escreveu");
        bia.send_direct(me, "direta do teste vivo").await.expect("B mandou a direta");

        let mut readings = Vec::new();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);

        while readings.len() < 2 && std::time::Instant::now() < until {
            let Ok(line) = heard.recv_timeout(std::time::Duration::from_millis(200)) else {
                continue;
            };
            let update: Value = serde_json::from_str(&line).expect("json");
            let event = update["event"].as_str().unwrap_or_default().to_owned();

            if event == "MessageSent" || event == "DirectMessageCreated" {
                readings.push((event, read_line(&update, me)));
            }
        }

        live.close();

        let message = readings.iter().find(|(event, _)| event == "MessageSent").expect("a mensagem do canal chegou");
        let direct = readings.iter().find(|(event, _)| event == "DirectMessageCreated").expect("a direta chegou");

        assert_eq!(message.1.messages_of.as_deref(), Some(channel.as_str()));
        assert_eq!(message.1.chime, Some(Chime::Message));
        assert_eq!(direct.1.direct_with, Some(other.id));
        assert!(direct.1.notice.as_ref().is_some_and(|notice| notice.text.ends_with("direta do teste vivo")));
    }

    fn read_line(update: &Value, me: i64) -> Reading {
        read(update["event"].as_str().unwrap_or_default(), update["channel"].as_str(), &update["data"], me, None)
    }

    #[test]
    fn the_presence_identity_becomes_the_account_id() {
        assert_eq!(person_id("user:42"), Some(42));
        assert_eq!(person_id("42"), Some(42));
        assert_eq!(person_id("guest:abc"), None);
    }
}
