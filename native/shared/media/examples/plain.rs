//! Prova que o SFU recebe mesmo o que este lado manda por RTP puro.
//!
//! Os testes de unidade já mostram que um quadro vira vários pacotes protegidos num
//! socket, mas pacote que sai não é pacote entendido: o SSRC, o tipo de payload e a
//! chave SRTP têm de bater com o que o servidor recebeu. O único jeito de saber é
//! perguntar a ele, então isto entra numa sala, declara a transmissão, manda H.264 de
//! mentira e espera o servidor dizer que está recebendo.
//!
//! cargo run -p media --example plain -- <ws-url> <sala>
//!
//! A sala é um código de 12 caracteres a-z0-9, o mesmo que o app sorteia.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use media::{EncodedFrame, PlainSender, Source};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

/// Uma unidade NAL grande o bastante para ser quebrada, exercitando a fragmentação.
fn frame(keyframe: bool, size: usize) -> EncodedFrame {
    let mut data = vec![0, 0, 0, 1, if keyframe { 0x65 } else { 0x41 }];

    data.extend(std::iter::repeat_n(0x5A, size));

    EncodedFrame {
        data,
        keyframe,
        timestamp_ns: 0,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let url = args.next().context("usage: plain <ws-url> <sala>")?;
    let room = args.next().context("usage: plain <ws-url> <sala>")?;

    let (mut socket, _) = tokio_tungstenite::connect_async(&url)
        .await
        .context("could not open the SFU WebSocket")?;

    let mut id = 0;

    let mut call = |action: &str, data: Value| {
        id += 1;

        Message::Text(
            json!({ "id": id, "action": action, "data": data })
                .to_string()
                .into(),
        )
    };

    socket
        .send(call("join", json!({ "room": room, "name": "plain-check" })))
        .await?;

    // A base dos SSRC vale para esta transmissão inteira: é ela que vai na oferta e a
    // mesma que numera os pacotes.
    let base = PlainSender::random_ssrc_base();
    let mut sender: Option<PlainSender> = None;
    let mut pending_key: Option<[u8; 30]> = None;
    let mut producer = String::new();
    let mut active = false;
    let mut sent = 0u32;

    let deadline = tokio::time::sleep(Duration::from_secs(20));
    tokio::pin!(deadline);

    let mut tick = tokio::time::interval(Duration::from_millis(33));

    loop {
        tokio::select! {
            _ = &mut deadline => break,

            _ = tick.tick(), if sender.is_some() => {
                let sender = sender.as_mut().expect("checked by the guard");

                // Keyframe primeiro: sem ele o servidor não tem o que pontuar.
                sender.send_frame(Source::Screen, frame(sent.is_multiple_of(60), 4_000), 30.0)?;
                sent += 1;
            }

            message = socket.next() => {
                let Some(message) = message else { break };
                let Message::Text(text) = message? else { continue };
                let payload: Value = serde_json::from_str(&text)?;

                if payload["event"] == "producerActive" && payload["data"]["producerId"] == producer.as_str() {
                    active = true;
                    break;
                }

                if payload["ok"] == false {
                    bail!("the SFU refused: {}", payload["error"]);
                }

                // A resposta do join é a primeira com id; depois dela vem o ingest.
                if payload["id"] == 1 {
                    println!("joined the room, declaring the broadcast…");

                    let key = PlainSender::generate_key();
                    let key_base64 = base64::Engine::encode(
                        &base64::engine::general_purpose::STANDARD,
                        key,
                    );

                    socket.send(call("producePlain", json!({
                        "kind": "video",
                        "source": "screen",
                        "rtpParameters": PlainSender::rtp_parameters(Source::Screen, base),
                        "srtpParameters": {
                            "cryptoSuite": PlainSender::CRYPTO_SUITE,
                            "keyBase64": key_base64,
                        },
                    }))).await?;

                    // Guardada para montar o remetente com a mesma chave quando o
                    // servidor responder com o endereço.
                    pending_key = Some(key);

                    continue;
                }

                if payload["id"] == 2 {
                    let target = &payload["data"];

                    producer = target["producerId"]
                        .as_str()
                        .ok_or_else(|| anyhow!("answer without a producer id"))?
                        .to_owned();

                    let address = format!(
                        "{}:{}",
                        target["ip"].as_str().unwrap_or("127.0.0.1"),
                        target["port"].as_u64().unwrap_or(0),
                    );

                    let key = pending_key
                        .take()
                        .ok_or_else(|| anyhow!("key lost between the request and the answer"))?;

                    println!("sending RTP to {address}");
                    sender = Some(PlainSender::connect(address.as_str(), &key, None, base)?);
                }
            }
        }
    }

    println!("frames sent: {sent}");

    if !active {
        bail!("the server never reported receiving — check the port, the SSRC or the key");
    }

    println!("the SFU confirmed it is receiving the broadcast");

    Ok(())
}
