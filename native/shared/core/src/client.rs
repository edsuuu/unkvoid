//! O cliente do SFU: uma conexão, chamadas que esperam resposta, e eventos que chegam
//! sozinhos.
//!
//! Quem chama `call` fica esperando em um canal próprio, registrado pelo `id` do pedido. A
//! leitura do socket acontece numa tarefa só, que decide se o que chegou é resposta de
//! alguém ou evento para a fila — sem isso, duas chamadas em voo disputariam a mesma
//! mensagem.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{Event, Incoming, Request, ServerError};

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Incoming>>>>;

/// Servidor que aceita a conexão e não completa o aperto de mão deixa o cliente
/// pendurado para sempre — e a tela que espera por ele nunca sai do lugar. Dez
/// segundos é muito mais do que uma conexão boa precisa e pouco para quem espera.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Quanto uma ação espera pela resposta. O mesmo prazo do app de hoje: sem ele, um servidor
/// que aceita o socket e não responde prende para sempre quem perguntou.
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

pub struct SfuClient {
    outgoing: mpsc::UnboundedSender<Message>,
    pending: Pending,
    next_id: AtomicU64,
}

impl SfuClient {
    /// Conecta e devolve o cliente junto com a fila de eventos. A fila é do chamador: o que
    /// ele não consumir se acumula, e é ele quem decide o que fazer com cada evento.
    pub async fn connect(url: &str) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Event>)> {
        let (socket, _) = timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
            .await
            .map_err(|_| anyhow!("o servidor não completou a conexão a tempo"))?
            .with_context(|| format!("não deu para conectar em {url}"))?;

        let (mut sink, mut stream) = socket.split();
        let (outgoing, mut to_send) = mpsc::unbounded_channel::<Message>();
        let (events, incoming_events) = mpsc::unbounded_channel::<Event>();

        let client = Arc::new(Self {
            outgoing,
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
        });

        tokio::spawn(async move {
            while let Some(message) = to_send.recv().await {
                if sink.send(message).await.is_err() {
                    break;
                }
            }
        });

        let pending = Arc::clone(&client.pending);

        tokio::spawn(async move {
            while let Some(Ok(message)) = stream.next().await {
                let Message::Text(raw) = message else {
                    continue;
                };

                let Ok(incoming) = Incoming::parse(&raw) else {
                    tracing::warn!("mensagem ilegível do servidor, descartada");

                    continue;
                };

                if let Some(id) = incoming.id {
                    if let Some(waiting) = pending.lock().await.remove(&id) {
                        let _ = waiting.send(incoming);
                    }

                    continue;
                }

                if let Some(event) = incoming.into_event() {
                    let _ = events.send(event);
                }
            }

            // O socket caiu: ninguém mais responde, e quem está esperando precisa saber
            // disso agora em vez de ficar pendurado para sempre.
            pending.lock().await.clear();
        });

        Ok((client, incoming_events))
    }

    /// Manda a ação e espera a resposta daquele `id`.
    pub async fn call(&self, action: &str, data: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();

        self.pending.lock().await.insert(id, sender);

        let body = serde_json::to_string(&Request { id, action, data })?;

        if self.outgoing.send(Message::text(body)).is_err() {
            self.pending.lock().await.remove(&id);

            return Err(anyhow!("a conexão com o servidor está fechada"));
        }

        let Ok(reply) = timeout(REPLY_TIMEOUT, receiver).await else {
            self.pending.lock().await.remove(&id);
            tracing::warn!(action, "o servidor não respondeu no prazo");

            return Err(anyhow::Error::new(crate::failure::Failure::Unreachable));
        };

        let reply = reply.map_err(|_| anyhow!("o servidor não respondeu a {action}"))?;

        if reply.ok == Some(true) {
            return Ok(reply.data.unwrap_or(Value::Null));
        }

        Err(ServerError {
            action: action.to_owned(),
            status: reply.status.unwrap_or(500),
            message: reply.error.unwrap_or_else(|| "erro sem mensagem".into()),
        }
        .into())
    }

    pub async fn identify(&self, token: &str) -> Result<Value> {
        self.call(crate::protocol::action::IDENTIFY, json!({ "token": token }))
            .await
    }

    pub async fn subscribe(&self, channel: &str) -> Result<Value> {
        self.call(
            crate::protocol::action::SUBSCRIBE,
            json!({ "channel": channel }),
        )
        .await
    }

    pub async fn unsubscribe(&self, channel: &str) -> Result<Value> {
        self.call(
            crate::protocol::action::UNSUBSCRIBE,
            json!({ "channel": channel }),
        )
        .await
    }
}
