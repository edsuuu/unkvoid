//! Proves that the SFU actually receives what this side sends over plain RTP.
//!
//! The unit tests already show that a frame becomes several protected packets on a real
//! socket, but a packet that leaves is not a packet that is understood: the SSRC, the
//! payload type and the SRTP key all have to match what the server was told. The only
//! way to know is to ask the server, so this joins a room, declares the broadcast, sends
//! synthetic H.264, and waits for the server to say it is receiving.
//!
//! cargo run -p media --example plain -- <ws-url> <token>
//!
//! The token is the same one the web app mints for a voice channel.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use media::{EncodedFrame, PlainSender};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

/// A NAL unit big enough to be split, so fragmentation is exercised too.
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
    let url = args.next().context("usage: plain <ws-url> <token>")?;
    let token = args.next().context("usage: plain <ws-url> <token>")?;

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

    socket.send(call("join", json!({ "token": token }))).await?;

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

                // A keyframe first: without it the server has nothing to score.
                sender.send_frame(&frame(sent.is_multiple_of(60), 4_000), 30.0)?;
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

                // The join reply is the first one with an id; ask for the ingest next.
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
                        "rtpParameters": PlainSender::rtp_parameters("video"),
                        "srtpParameters": {
                            "cryptoSuite": PlainSender::CRYPTO_SUITE,
                            "keyBase64": key_base64,
                        },
                    }))).await?;

                    // Kept so the sender can be built with the same key once the server
                    // answers with the address.
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
                    sender = Some(PlainSender::connect(address.as_str(), &key)?);
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
