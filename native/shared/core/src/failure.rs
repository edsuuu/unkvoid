//! O que a pessoa pode ver quando algo falha.
//!
//! **Nunca sai daqui caminho, endereço, código de status ou mensagem de servidor.** Quem
//! está na tela não tem o que fazer com `POST /api/sfu/authorize respondeu 403`: aquilo é
//! para o log, e mostrar na janela ainda entrega a quem não devia um mapa de como o serviço
//! é feito por dentro.
//!
//! A interface recebe um código destes e escreve a frase. O detalhe técnico vai para o
//! `tracing`, que termina em arquivo.

use serde::Serialize;

use crate::protocol::ServerError;

/// Os motivos que a interface sabe explicar. Poucos de propósito: cada um vira uma frase
/// que a pessoa entende e sobre a qual ela pode agir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Failure {
    /// Não deu para falar com o servidor: sem internet, servidor fora, ou caiu no meio.
    Unreachable,
    /// A sessão não vale mais. Entrar de novo resolve.
    SignedOut,
    /// A pessoa não pode fazer isso — canal oculto, sem cargo, expulsa.
    NotAllowed,
    /// O que foi pedido não existe mais: sala fechada, canal apagado, mensagem removida.
    Gone,
    /// O que foi digitado não serve.
    Invalid,
    /// O servidor quebrou. Não há o que a pessoa faça além de tentar de novo.
    ServerBroke,
    /// Passou do limite de tentativas; esperar um pouco resolve.
    TooFast,
}

impl Failure {
    /// Traduz a recusa do servidor sem deixar o status vazar para a tela.
    pub fn from_status(status: u16) -> Self {
        match status {
            401 => Self::SignedOut,
            403 => Self::NotAllowed,
            404 | 410 => Self::Gone,
            400 | 422 => Self::Invalid,
            429 => Self::TooFast,
            502..=504 => Self::Unreachable,
            _ => Self::ServerBroke,
        }
    }

    /// O que a interface recebe. O erro cru fica no log e não atravessa a ABI.
    pub fn from_error(failure: &anyhow::Error) -> Self {
        if let Some(known) = failure.downcast_ref::<Self>() {
            return *known;
        }

        match failure.downcast_ref::<ServerError>() {
            Some(server) => {
                tracing::warn!(action = %server.action, status = server.status, message = %server.message, "o servidor recusou");

                Self::from_status(server.status)
            }
            None => {
                tracing::warn!(%failure, "a chamada não chegou ao servidor");

                Self::Unreachable
            }
        }
    }
}

/// O nome do motivo, e só ele — é o que vai para o log. A frase que a pessoa lê é da
/// interface, e escrevê-la aqui obrigaria as três a concordar.
impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}", self)
    }
}

impl std::error::Error for Failure {}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::anyhow;

    #[test]
    fn every_status_becomes_something_the_person_can_act_on() {
        assert_eq!(Failure::from_status(401), Failure::SignedOut);
        assert_eq!(Failure::from_status(403), Failure::NotAllowed);
        assert_eq!(Failure::from_status(404), Failure::Gone);
        assert_eq!(Failure::from_status(422), Failure::Invalid);
        assert_eq!(Failure::from_status(429), Failure::TooFast);
        assert_eq!(Failure::from_status(500), Failure::ServerBroke);
    }

    #[test]
    fn a_network_failure_is_unreachable_and_not_a_server_error() {
        assert_eq!(Failure::from_error(&anyhow!("dns falhou")), Failure::Unreachable);
    }

    #[test]
    fn a_reason_decided_earlier_survives_the_trip() {
        let carried = anyhow::Error::new(Failure::NotAllowed).context("o token de voz");

        assert_eq!(Failure::from_error(&carried), Failure::NotAllowed);
    }

    #[test]
    fn the_serialised_name_carries_no_technical_detail() {
        // É isto que a interface recebe: um nome, e nada mais.
        for failure in [Failure::NotAllowed, Failure::Unreachable, Failure::ServerBroke] {
            let json = serde_json::to_string(&failure).expect("serialize");

            assert!(!json.contains("http"), "vazou endereço: {json}");
            assert!(!json.chars().any(|letter| letter.is_ascii_digit()), "vazou número: {json}");
        }
    }

    #[test]
    fn a_server_refusal_never_leaks_its_message_to_the_interface() {
        let refusal = ServerError {
            action: "subscribe".into(),
            status: 403,
            message: "not authorized for channel.01jbqz at https://unkvoid.com/api".into(),
        };

        let failure = Failure::from_error(&anyhow::Error::new(refusal));
        let json = serde_json::to_string(&failure).expect("serialize");

        assert_eq!(failure, Failure::NotAllowed);
        assert!(!json.contains("unkvoid.com"), "o endereço chegou à tela: {json}");
        assert!(!json.contains("channel"), "o nome do canal chegou à tela: {json}");
        assert!(!json.contains("403"), "o status chegou à tela: {json}");
    }
}
