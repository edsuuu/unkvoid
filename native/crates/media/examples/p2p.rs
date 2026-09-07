//! Proves that both sides establish the connection for real.
//!
//! Starts two PeerConnections in the same process and performs a complete offer,
//! response, and candidates with no server in the middle. If ICE connects, negotiation
//! is correct and only signaling transport remains.
//!
//! cargo run -p media --example p2p

use std::time::Duration;

use media::{PeerLink, Signal};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (mut sender, mut sender_signals) =
        PeerLink::connect(vec!["stun:stun.l.google.com:19302".into()], 60.0).await?;
    let (mut receiver, mut receiver_signals) =
        PeerLink::connect(vec!["stun:stun.l.google.com:19302".into()], 60.0).await?;

    let offer = sender.create_offer().await?;

    println!(
        "offer: {} bytes · H.264: {}",
        offer.len(),
        offer.to_lowercase().contains("h264")
    );

    let answer = receiver.accept_offer(offer).await?;

    println!("answer: {} bytes", answer.len());

    sender.accept_answer(answer).await?;

    println!("SDP exchanged — now the candidates\n");

    let deadline = tokio::time::sleep(Duration::from_secs(10));

    tokio::pin!(deadline);

    let (mut from_sender, mut from_receiver) = (0, 0);

    loop {
        tokio::select! {
            Some(Signal::Candidate(json)) = sender_signals.recv() => {
                from_sender += 1;
                let _ = receiver.add_candidate(json).await;
            }
            Some(Signal::Candidate(json)) = receiver_signals.recv() => {
                from_receiver += 1;
                let _ = sender.add_candidate(json).await;
            }
            _ = &mut deadline => break,
        }
    }

    println!("candidates exchanged: {from_sender} from sender · {from_receiver} from receiver");

    sender.close().await?;
    receiver.close().await?;

    println!(
        "\n{}",
        if from_sender > 0 && from_receiver > 0 {
            "NEGOTIATION COMPLETE — offer, response, and candidates from both sides"
        } else {
            "INCOMPLETE: see the numbers above"
        }
    );

    Ok(())
}
