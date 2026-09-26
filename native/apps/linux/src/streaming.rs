//! O que liga a sinalização à mídia: publicar o que sai, consumir o que entra.
//!
//! O SFU não adivinha nada. Antes do primeiro pacote ele já sabe o SSRC, o tipo de payload
//! e a chave SRTP de cada origem — é o que o `producePlain` leva. No outro sentido, o
//! `consumePlain` devolve para onde mandar e com que chave abrir.
//!
//! Regra de negócio nenhuma mora aqui: quem diz o que esta sessão pode produzir é o `can`
//! do `join`, que é do servidor.

use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use base64::Engine;
use capture::CaptureConfig;
use core_app::Session;
use core_app::models::ProducerInfo;
use core_app::protocol::action;
use media::Source;
use serde_json::{Value, json};

use crate::sending::Sending;
use crate::watching::{Incoming, Watching};

/// A única suíte combinada com o servidor, dos dois lados.
const CRYPTO_SUITE: &str = "AES_CM_128_HMAC_SHA1_80";

/// Uma transmissão sendo assistida, do jeito que a janela desenha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub producer_id: String,
    pub label: String,
    /// Câmera é cartão pequeno; tela é o palco.
    pub camera: bool,
}

/// O que esta pessoa está mandando, e o que ela tem permissão de mandar. A interface só
/// esconde botão — quem autoriza é o `can` do servidor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mine {
    pub sharing: bool,
    pub mic: bool,
    pub mic_muted: bool,
    pub camera: bool,
    pub can_share: bool,
    pub can_speak: bool,
    pub can_video: bool,
}

/// Abre uma origem no servidor e só então começa a capturar: sem o `producePlain` não há
/// porta para onde mandar, e o quadro sairia no vazio.
pub async fn publish(
    session: &Arc<Session>,
    sending: &Arc<Mutex<Sending>>,
    key: Source,
    config: CaptureConfig,
    video: Option<Source>,
    audio: Option<Source>,
) -> Result<()> {
    if lock(sending).is_live(key) {
        return Ok(());
    }

    let mut producers = Vec::new();
    let mut last = Value::Null;

    for source in [video, audio].into_iter().flatten() {
        let offer = lock(sending).offer(source);
        let mut request = json!({
            "kind": if source.is_video() { "video" } else { "audio" },
            "source": name_of(source),
        });

        merge(&mut request, offer);

        let answer = session.client().call(action::PRODUCE_PLAIN, request).await?;

        producers.push(text(&answer, "producerId"));
        last = answer;
    }

    let address = format!("{}:{}", last["ip"].as_str().unwrap_or_default(), last["port"]);
    let server_key = decode(&last["srtpParameters"]["keyBase64"]);

    // Fora do cadeado da sessão e antes da captura: resolver o endereço pode ir ao DNS.
    lock(sending).use_sfu(&address, server_key)?;
    lock(sending).start(key, config, video, audio, producers)?;

    Ok(())
}

/// Para de capturar e fecha os producers que o servidor ainda tem abertos.
pub async fn unpublish(session: &Arc<Session>, sending: &Arc<Mutex<Sending>>, key: Source) {
    let producers = lock(sending).stop(key);

    for producer_id in producers {
        if let Err(failure) =
            session.client().call(action::CLOSE_PRODUCER, json!({ "producerId": producer_id })).await
        {
            tracing::warn!(%failure, producer = %producer_id, "o producer não fechou no servidor");
        }
    }
}

/// Assiste a uma transmissão de outra pessoa. O `consumePlain` devolve por onde ela vem e a
/// chave para abri-la; o `resumeConsumer` é o que solta o primeiro pacote.
pub async fn consume(
    session: &Arc<Session>,
    watching: &Arc<Mutex<Watching>>,
    producer: &ProducerInfo,
) -> Result<()> {
    if lock(watching).is_watching(&producer.producer_id) {
        return Ok(());
    }

    let key = lock(watching).key();
    let answer = session
        .client()
        .call(
            action::CONSUME_PLAIN,
            json!({
                "producerId": producer.producer_id,
                "srtpParameters": {
                    "cryptoSuite": CRYPTO_SUITE,
                    "keyBase64": base64::engine::general_purpose::STANDARD.encode(key),
                },
            }),
        )
        .await?;

    let consumer_id = text(&answer, "consumerId");
    let address = format!("{}:{}", answer["ip"].as_str().unwrap_or_default(), answer["port"]);
    let server_key =
        decode(&answer["srtpParameters"]["keyBase64"]).ok_or_else(|| anyhow!("consumidor sem chave"))?;
    let payload_type = answer["payloadType"].as_u64().unwrap_or_default() as u8;
    let ssrc = answer["ssrc"].as_u64().map(|ssrc| ssrc as u32);
    let kind = answer["kind"].as_str().unwrap_or(&producer.kind).to_owned();
    let source = answer["source"].as_str().unwrap_or(&producer.source).to_owned();

    let started = lock(watching).start(Incoming {
        producer_id: producer.producer_id.clone(),
        kind: &kind,
        address: &address,
        server_key: &server_key,
        payload_type,
        ssrc,
        always_muted: source == "screenAudio",
        rtx: core_app::watching::rtx_of(&answer),
    });

    if let Err(failure) = started {
        let _ = session.client().call(action::CLOSE_CONSUMER, json!({ "consumerId": consumer_id })).await;

        return Err(failure);
    }

    session.client().call(action::RESUME_CONSUMER, json!({ "consumerId": consumer_id })).await?;

    Ok(())
}

/// Os cartões que a janela desenha: uma transmissão de vídeo de outra pessoa, por cartão.
pub fn tiles(session: &Arc<Session>, watching: &Arc<Mutex<Watching>>) -> Vec<Tile> {
    let watching = lock(watching);

    session
        .peers()
        .iter()
        .filter(|peer| !peer.self_peer)
        .flat_map(|peer| {
            peer.producers.iter().filter_map(|producer| {
                let camera = producer.source == "camera";

                if producer.kind != "video" || !watching.is_watching(&producer.producer_id) {
                    return None;
                }

                Some(Tile {
                    producer_id: producer.producer_id.clone(),
                    label: peer.name.clone(),
                    camera,
                })
            })
        })
        .collect()
}

pub fn mine(session: &Arc<Session>, sending: &Arc<Mutex<Sending>>) -> Mine {
    let sending = lock(sending);

    Mine {
        sharing: sending.is_live(Source::Screen),
        mic: sending.is_live(Source::Mic),
        mic_muted: sending.is_muted(Source::Mic),
        camera: sending.is_live(Source::Camera),
        can_share: session.can("stream"),
        can_speak: session.can("speak"),
        can_video: session.can("video"),
    }
}

/// O nome que atravessa a rede. O `Source` não o carrega de volta, e escrevê-lo à mão em
/// cada chamada daria quatro lugares para errar.
fn name_of(source: Source) -> &'static str {
    match source {
        Source::Screen => "screen",
        Source::ScreenAudio => "screenAudio",
        Source::Camera => "camera",
        Source::Mic => "mic",
    }
}

/// O `producePlain` recebe o pedido e a oferta no mesmo objeto (`{kind, source, ...offer}`).
fn merge(request: &mut Value, offer: Value) {
    let (Some(request), Some(offer)) = (request.as_object_mut(), offer.as_object()) else {
        return;
    };

    for (field, value) in offer {
        request.insert(field.clone(), value.clone());
    }
}

fn text(answer: &Value, field: &str) -> String {
    answer[field].as_str().unwrap_or_default().to_owned()
}

fn decode(value: &Value) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(value.as_str()?).ok()
}

/// Um `Mutex` envenenado aqui é uma thread de captura que caiu. Seguir com o que se tem é
/// melhor do que derrubar a janela de quem está na sala.
fn lock<T>(cell: &Arc<Mutex<T>>) -> std::sync::MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offer_and_the_request_go_up_in_the_same_object() {
        let mut request = json!({ "kind": "video", "source": "screen" });

        merge(&mut request, json!({ "rtpParameters": { "ssrc": 7 }, "srtpParameters": {} }));

        assert_eq!(request["source"], "screen");
        assert_eq!(request["rtpParameters"]["ssrc"], 7);
        assert!(request.get("srtpParameters").is_some());
    }

    #[test]
    fn every_source_has_the_name_the_sfu_expects() {
        // O nome volta ao `Source` do outro lado: errar um aqui produziria um producer que
        // o servidor aceita e ninguém consegue assistir.
        for source in [Source::Screen, Source::ScreenAudio, Source::Camera, Source::Mic] {
            assert_eq!(Source::parse(name_of(source)), Some(source));
        }
    }
}
