//! Quem está na sala, agora — e o socket que se segura de pé sozinho.
//!
//! O SFU não manda a lista de novo a cada mudança: manda o que mudou. Manter a lista em dia
//! é decidir, evento a evento, o que aconteceu com quem — e isso é a mesma decisão nas três
//! interfaces. Escrita aqui uma vez, elas só redesenham o que `peers()` devolver.
//!
//! A sessão também **vigia a si mesma**: `ping` a cada 5 s, e o socket que emudecer volta
//! com a `resumeKey`, sem a sala ver ninguém sair. Sem isso o TCP da sinalização podia
//! sumir com a mídia junto e o app só descobrir minutos depois.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::client::SfuClient;
use crate::models::{JoinResponse, Peer, ProducerInfo, RoomIdentity};
use crate::protocol::{Event, action, local};
use crate::reconnect::Backoff;

/// Quem se apresenta ao SFU, **de novo a cada entrada**. É função, e não valor, porque o
/// token de voz vale 60 s: guardar o primeiro faria toda reconexão levar um token vencido.
pub type Identity =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<RoomIdentity>> + Send>> + Send + Sync>;

/// De quanto em quanto tempo o app prova que a sinalização vive (`docs/CONTRATO.md`).
const PING_EVERY: Duration = Duration::from_secs(5);

/// Silêncio além disto é socket morto. Qualquer resposta, até erro, conta como vivo.
const PING_PATIENCE: Duration = Duration::from_secs(10);

/// A lista de quem está na sala, e o que cada evento faz com ela. Separada do socket de
/// propósito: é a parte que se prova sem servidor nenhum.
#[derive(Debug, Default)]
pub struct Roster {
    peers: Vec<Peer>,
}

impl Roster {
    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }

    /// A pessoa não vem na lista que o servidor manda — ele não se inclui na resposta — e
    /// ela precisa se ver na sala.
    pub fn reset(&mut self, joined: &JoinResponse) {
        self.peers = vec![Peer {
            peer_id: joined.peer_id.clone(),
            user_id: None,
            name: joined.name.clone(),
            producers: Vec::new(),
            reconnecting: false,
            self_peer: true,
        }];

        self.peers.extend(joined.peers.iter().cloned());
    }

    /// Devolve `true` quando a lista mudou — a interface só redesenha nesse caso.
    pub fn apply(&mut self, event: &Event) -> bool {
        let data = &event.data;
        let peer_id = data["peerId"].as_str().unwrap_or_default();

        if peer_id.is_empty() {
            return false;
        }

        match event.name.as_str() {
            "peerJoined" => self.arrived(peer_id, data),
            "peerLeft" | "peerKicked" => {
                let before = self.peers.len();

                self.peers.retain(|peer| peer.peer_id != peer_id);

                self.peers.len() != before
            }
            "newProducer" => self.produced(peer_id, data),
            "producerClosed" => {
                let producer_id = data["producerId"].as_str().unwrap_or_default();
                let Some(peer) = self.find(peer_id) else {
                    return false;
                };
                let before = peer.producers.len();

                peer.producers.retain(|producer| producer.producer_id != producer_id);

                peer.producers.len() != before
            }
            "producerPaused" | "producerResumed" => {
                let paused = event.name == "producerPaused";
                let producer_id = data["producerId"].as_str().unwrap_or_default();
                let Some(peer) = self.find(peer_id) else {
                    return false;
                };

                peer.producers
                    .iter_mut()
                    .find(|producer| producer.producer_id == producer_id)
                    .filter(|producer| producer.paused != paused)
                    .map(|producer| producer.paused = paused)
                    .is_some()
            }
            "peerConnectionLost" | "peerReconnected" => {
                let lost = event.name == "peerConnectionLost";
                let Some(peer) = self.find(peer_id) else {
                    return false;
                };

                if peer.reconnecting == lost {
                    return false;
                }

                peer.reconnecting = lost;

                true
            }
            _ => false,
        }
    }

    fn arrived(&mut self, peer_id: &str, data: &Value) -> bool {
        if self.peers.iter().any(|peer| peer.peer_id == peer_id) {
            return false;
        }

        self.peers.push(Peer {
            peer_id: peer_id.to_owned(),
            user_id: data["userId"].as_str().map(str::to_owned),
            name: data["name"].as_str().unwrap_or_default().to_owned(),
            producers: Vec::new(),
            reconnecting: false,
            self_peer: false,
        });

        true
    }

    fn produced(&mut self, peer_id: &str, data: &Value) -> bool {
        let producer_id = data["producerId"].as_str().unwrap_or_default().to_owned();
        let kind = data["kind"].as_str().unwrap_or_default().to_owned();
        let source = data["source"].as_str().unwrap_or_default().to_owned();
        let Some(peer) = self.find(peer_id) else {
            return false;
        };

        if peer.producers.iter().any(|producer| producer.producer_id == producer_id) {
            return false;
        }

        peer.producers.push(ProducerInfo { producer_id, kind, source, paused: false });

        true
    }

    fn find(&mut self, peer_id: &str) -> Option<&mut Peer> {
        self.peers.iter_mut().find(|peer| peer.peer_id == peer_id)
    }
}

#[derive(Default)]
struct State {
    roster: Roster,
    /// Sem ela a volta depois de uma queda seria entrada nova, e a mídia morreria junto.
    resume_key: Option<String>,
    can: Vec<String>,
    /// A última entrada foi retomada (mídia intacta) ou nova (tudo caiu do lado de lá).
    resumed: bool,
}

/// Uma sala aberta: o socket, a lista de quem está e o que esta sessão pode fazer.
pub struct Session {
    /// Troca inteiro na reconexão — por isso `Mutex`, e não um `Arc` fixo.
    client: Mutex<Arc<SfuClient>>,
    url: String,
    /// O que a sala é: o código, sem conta; o ULID do canal de voz, com conta.
    room: String,
    identity: Identity,
    state: Mutex<State>,
    /// Quem saiu de propósito não volta: sem isto, `leave()` disparava a reconexão.
    left: AtomicBool,
}

impl Session {
    /// Abre o socket e entra. Devolve também a fila de eventos: quem chama joga cada um em
    /// `apply` e redesenha se ele disser que mudou alguma coisa.
    ///
    /// A fila **sobrevive à queda**: quem escuta não reabre nada, só vê passar
    /// `sessionLost`, `sessionRejoined` ou `sessionGone`.
    pub async fn join(
        url: &str,
        room: &str,
        identity: Identity,
    ) -> Result<(Arc<Self>, UnboundedReceiver<Event>)> {
        let (client, incoming) = SfuClient::connect(url).await?;

        let session = Arc::new(Self {
            client: Mutex::new(client),
            url: url.to_owned(),
            room: room.to_owned(),
            identity,
            state: Mutex::new(State::default()),
            left: AtomicBool::new(false),
        });

        session.request_join(false).await?;

        let (events, queue) = unbounded_channel();

        tokio::spawn({
            let session = Arc::clone(&session);

            async move { session.supervise(incoming, events).await }
        });

        Ok((session, queue))
    }

    async fn request_join(&self, resume: bool) -> Result<JoinResponse> {
        let mut data = serde_json::to_value((self.identity)().await?)?;

        if let Value::Object(fields) = &mut data {
            fields.insert("resumeKey".into(), json!(self.state().resume_key));
            fields.insert("resume".into(), json!(resume));
        }

        let answer = self.client().call(action::JOIN, data).await?;
        let joined: JoinResponse = serde_json::from_value(answer)?;
        let mut state = self.state();

        state.resume_key = joined.resume_key.clone();
        state.can = joined.can.clone();
        state.resumed = joined.resumed;
        state.roster.reset(&joined);

        Ok(joined)
    }

    /// Repassa os eventos do socket e, entre um e outro, prova que ele vive. Sai quando o
    /// socket morre (para a reconexão tentar) ou quando alguém saiu da sala de propósito.
    async fn supervise(
        self: Arc<Self>,
        mut incoming: UnboundedReceiver<Event>,
        events: UnboundedSender<Event>,
    ) {
        loop {
            if !self.forward(&mut incoming, &events).await {
                return;
            }

            if events.send(Event::local(local::SESSION_LOST)).is_err() {
                return;
            }

            let Some(fresh) = self.reconnect().await else {
                let _ = events.send(Event::local(local::SESSION_GONE));

                return;
            };

            incoming = fresh;

            let _ = events.send(Event::local(local::SESSION_REJOINED));
        }
    }

    /// `false` quando não há o que reconectar: a fila do app fechou ou a saída foi pedida.
    async fn forward(
        &self,
        incoming: &mut UnboundedReceiver<Event>,
        events: &UnboundedSender<Event>,
    ) -> bool {
        let mut beat = tokio::time::interval(PING_EVERY);

        // O primeiro tick de um `interval` sai na hora, e um ping no instante do `join` não
        // prova nada.
        beat.tick().await;

        loop {
            tokio::select! {
                event = incoming.recv() => match event {
                    Some(event) => {
                        if events.send(event).is_err() {
                            return false;
                        }
                    }
                    None => return !self.left.load(Ordering::Relaxed),
                },
                _ = beat.tick() => {
                    let client = self.client();

                    // Resposta de erro também prova que o socket vive: só o silêncio derruba.
                    if tokio::time::timeout(PING_PATIENCE, client.call(action::PING, json!({})))
                        .await
                        .is_err()
                    {
                        tracing::warn!("a sinalização emudeceu: o socket vai voltar");

                        return !self.left.load(Ordering::Relaxed);
                    }
                }
            }
        }
    }

    /// Volta para a MESMA sessão do servidor (`resume`), que mantém a mídia de pé. Se a
    /// carência já expirou lá, entra de novo — que é pior, mas é voltar.
    async fn reconnect(&self) -> Option<UnboundedReceiver<Event>> {
        let mut backoff = Backoff::default();

        while let Some(wait) = backoff.next_delay() {
            tokio::time::sleep(wait).await;

            if self.left.load(Ordering::Relaxed) {
                return None;
            }

            let (client, incoming) = match SfuClient::connect(&self.url).await {
                Ok(connected) => connected,
                Err(failure) => {
                    tracing::warn!(%failure, attempt = backoff.attempt, "o socket não voltou");

                    continue;
                }
            };

            *self.client.lock().unwrap_or_else(PoisonError::into_inner) = client;

            if self.request_join(true).await.is_ok() {
                return Some(incoming);
            }

            match self.request_join(false).await {
                Ok(_) => return Some(incoming),
                Err(failure) => {
                    tracing::warn!(%failure, attempt = backoff.attempt, "a sala recusou a volta");
                }
            }
        }

        None
    }

    pub fn room(&self) -> &str {
        &self.room
    }

    pub fn peers(&self) -> Vec<Peer> {
        self.state().roster.peers().to_vec()
    }

    /// O que o token aceito agora permite. É ele, e não os bits do canal, que decide se o
    /// app liga o microfone, a câmera e a tela.
    pub fn can(&self, what: &str) -> bool {
        self.state().can.iter().any(|allowed| allowed == what)
    }

    pub fn resume_key(&self) -> Option<String> {
        self.state().resume_key.clone()
    }

    /// A última entrada foi retomada? Entrada nova (`false`) perdeu os producers do lado de
    /// lá, e quem transmitia precisa publicar de novo.
    pub fn resumed(&self) -> bool {
        self.state().resumed
    }

    pub fn client(&self) -> Arc<SfuClient> {
        Arc::clone(&self.client.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn apply(&self, event: &Event) -> bool {
        // A volta refez a lista inteira: quem desenha precisa redesenhar, mesmo que o
        // evento não fale de ninguém em especial.
        if event.name == local::SESSION_REJOINED {
            return true;
        }

        self.state().roster.apply(event)
    }

    pub async fn leave(&self) -> Result<()> {
        self.left.store(true, Ordering::Relaxed);
        self.client().call(action::LEAVE, json!({})).await?;

        Ok(())
    }

    /// Um `Mutex` envenenado aqui é outra thread que caiu no meio de uma escrita da lista.
    /// Derrubar o app por isso seria pior do que desenhar uma lista desatualizada.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster_with(names: &[&str]) -> Roster {
        Roster { peers: names.iter().map(|name| peer(name)).collect() }
    }

    fn peer(peer_id: &str) -> Peer {
        Peer {
            peer_id: peer_id.into(),
            user_id: None,
            name: peer_id.into(),
            producers: Vec::new(),
            reconnecting: false,
            self_peer: false,
        }
    }

    fn event(name: &str, data: Value) -> Event {
        Event { name: name.into(), channel: None, data }
    }

    #[test]
    fn the_person_sees_herself_in_the_room() {
        let joined: JoinResponse = serde_json::from_str(
            r#"{"peerId":"mine","name":"Ada","resumeKey":"k","peers":[{"peerId":"abc","name":"Grace"}],"can":["speak"]}"#,
        )
        .expect("parse");

        let mut roster = Roster::default();

        roster.reset(&joined);

        assert_eq!(roster.peers().len(), 2);
        assert!(roster.peers()[0].self_peer);
        assert_eq!(roster.peers()[1].name, "Grace");
    }

    #[test]
    fn someone_arriving_shows_up_once() {
        let mut roster = Roster::default();
        let arrived = event("peerJoined", json!({ "peerId": "abc", "userId": "user:1", "name": "Ada" }));

        assert!(roster.apply(&arrived));
        assert_eq!(roster.peers().len(), 1);

        // O mesmo evento repetido (a reconexão reproduz o que se perdeu) não duplica.
        assert!(!roster.apply(&arrived));
        assert_eq!(roster.peers().len(), 1);
    }

    #[test]
    fn someone_leaving_is_removed() {
        let mut roster = roster_with(&["abc", "xyz"]);

        assert!(roster.apply(&event("peerLeft", json!({ "peerId": "abc" }))));
        assert_eq!(roster.peers().len(), 1);
        assert!(!roster.apply(&event("peerLeft", json!({ "peerId": "abc" }))));
    }

    #[test]
    fn a_screen_producer_is_what_makes_someone_live() {
        let mut roster = roster_with(&["abc"]);
        let started = event(
            "newProducer",
            json!({ "peerId": "abc", "producerId": "p1", "kind": "video", "source": "screen" }),
        );

        assert!(roster.apply(&started));
        assert!(roster.peers()[0].sharing());
        assert!(!roster.apply(&started));

        assert!(roster.apply(&event("producerClosed", json!({ "peerId": "abc", "producerId": "p1" }))));
        assert!(!roster.peers()[0].sharing());
    }

    #[test]
    fn a_paused_microphone_is_seen_without_the_person_leaving() {
        let mut roster = roster_with(&["abc"]);

        roster.apply(&event(
            "newProducer",
            json!({ "peerId": "abc", "producerId": "p1", "kind": "audio", "source": "mic" }),
        ));

        let paused = event("producerPaused", json!({ "peerId": "abc", "producerId": "p1" }));

        assert!(roster.apply(&paused));
        assert!(roster.peers()[0].producers[0].paused);
        assert!(!roster.apply(&paused));
        assert!(roster.apply(&event("producerResumed", json!({ "peerId": "abc", "producerId": "p1" }))));
    }

    #[test]
    fn someone_who_dropped_stays_on_the_list_until_the_grace_runs_out() {
        let mut roster = roster_with(&["abc"]);

        assert!(roster.apply(&event("peerConnectionLost", json!({ "peerId": "abc" }))));
        assert!(roster.peers()[0].reconnecting, "whoever dropped must not vanish from the room");

        assert!(roster.apply(&event("peerReconnected", json!({ "peerId": "abc" }))));
        assert!(!roster.peers()[0].reconnecting);
    }

    #[test]
    fn an_event_about_a_stranger_changes_nothing() {
        let mut roster = roster_with(&["abc"]);

        assert!(!roster.apply(&event("newProducer", json!({ "peerId": "outro", "producerId": "p1" }))));
        assert!(!roster.apply(&event("coisaNova", json!({ "peerId": "abc" }))));
        assert!(!roster.apply(&event("peerLeft", json!({}))));
    }
}
