//! O que trafega no WebSocket do SFU.
//!
//! Duas coisas descem por ele, e o que as separa é o `id`: resposta a um pedido que este
//! app fez, ou evento que o servidor mandou por conta própria. O `id` é de quem pergunta,
//! e volta igual na resposta — é assim que duas chamadas em voo não trocam de resposta.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// As ações do SFU. Estão aqui como texto porque é assim que viajam, e errar o nome de uma
/// é um erro de compilação, não uma mensagem recusada em produção.
pub mod action {
    pub const JOIN: &str = "join";
    pub const LEAVE: &str = "leave";
    pub const PING: &str = "ping";
    pub const IDENTIFY: &str = "identify";
    pub const SUBSCRIBE: &str = "subscribe";
    pub const UNSUBSCRIBE: &str = "unsubscribe";
    pub const CREATE_TRANSPORT: &str = "createTransport";
    pub const CONNECT_TRANSPORT: &str = "connectTransport";
    pub const PRODUCE_PLAIN: &str = "producePlain";
    pub const CONSUME_PLAIN: &str = "consumePlain";
    pub const PAUSE_PRODUCER: &str = "pauseProducer";
    pub const RESUME_PRODUCER: &str = "resumeProducer";
    pub const CLOSE_PRODUCER: &str = "closeProducer";
    pub const PAUSE_CONSUMER: &str = "pauseConsumer";
    pub const RESUME_CONSUMER: &str = "resumeConsumer";
    pub const CLOSE_CONSUMER: &str = "closeConsumer";
    pub const REMOVE_PEER: &str = "removePeer";
}

/// Eventos que o servidor **não** manda: são desta máquina, sobre o socket dela. Entram na
/// mesma fila para a interface ter um caminho só entre "alguém chegou" e "a sala caiu".
pub mod local {
    pub const SESSION_LOST: &str = "sessionLost";
    pub const SESSION_REJOINED: &str = "sessionRejoined";
    /// Desistiu de voltar. Daqui não vem mais nada.
    pub const SESSION_GONE: &str = "sessionGone";
    /// O ida e volta até o SFU, em milissegundos, medido no ping que já é enviado de 5 em
    /// 5 s. O número vem em `data`, e é o que a barra da sala mostra.
    pub const PING_MEASURED: &str = "pingMeasured";
}

#[derive(Debug, Serialize)]
pub struct Request<'a> {
    pub id: u64,
    pub action: &'a str,
    pub data: Value,
}

/// O que o servidor manda. `id` presente é resposta; ausente é evento da sala.
#[derive(Debug, Deserialize)]
pub struct Incoming {
    #[serde(default)]
    pub id: Option<u64>,

    #[serde(default)]
    pub ok: Option<bool>,

    #[serde(default)]
    pub status: Option<u16>,

    #[serde(default)]
    pub error: Option<String>,

    #[serde(default)]
    pub data: Option<Value>,

    #[serde(default)]
    pub event: Option<String>,

    #[serde(default)]
    pub channel: Option<String>,
}

/// Um evento do servidor, já separado das respostas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    /// Só os eventos do chat têm canal; os da sala não.
    pub channel: Option<String>,
    pub data: Value,
}

/// O erro que o servidor devolve, com o status que ele mesmo escolheu.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{action} ({status}): {message}")]
pub struct ServerError {
    pub action: String,
    pub status: u16,
    pub message: String,
}

impl Event {
    pub fn local(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            channel: None,
            data: Value::Null,
        }
    }
}

impl Incoming {
    pub fn parse(raw: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(raw)
    }

    pub fn is_reply(&self) -> bool {
        self.id.is_some()
    }

    pub fn into_event(self) -> Option<Event> {
        Some(Event {
            name: self.event?,
            channel: self.channel,
            data: self.data.unwrap_or(Value::Null),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    #[test]
    fn the_id_is_what_tells_a_reply_from_an_event() {
        let reply =
            Incoming::parse(r#"{"id":7,"ok":true,"data":{"peerId":"abc"}}"#).expect("parse");

        assert!(reply.is_reply());
        assert_eq!(reply.id, Some(7));

        let event =
            Incoming::parse(r#"{"event":"peerJoined","data":{"peerId":"xyz"}}"#).expect("parse");

        assert!(!event.is_reply());
        assert_eq!(
            event.into_event(),
            Some(Event {
                name: "peerJoined".into(),
                channel: None,
                data: json!({ "peerId": "xyz" }),
            }),
        );
    }

    #[test]
    fn a_chat_event_carries_its_channel() {
        let raw = r#"{"event":"MessageSent","channel":"channel.5","data":{"body":"oi"}}"#;
        let event = Incoming::parse(raw)
            .expect("parse")
            .into_event()
            .expect("into event");

        assert_eq!(event.channel.as_deref(), Some("channel.5"));
        assert_eq!(event.data["body"], "oi");
    }

    #[test]
    fn an_error_reply_carries_status_and_message() {
        let raw = r#"{"id":3,"ok":false,"status":403,"error":"not authorized"}"#;
        let reply = Incoming::parse(raw).expect("parse");

        assert_eq!(reply.ok, Some(false));
        assert_eq!(reply.status, Some(403));
        assert_eq!(reply.error.as_deref(), Some("not authorized"));
    }

    #[test]
    fn an_unknown_field_does_not_break_parsing() {
        let raw = r#"{"event":"peerLeft","data":{},"coisaNova":123}"#;

        assert!(Incoming::parse(raw).is_ok());
    }
}
