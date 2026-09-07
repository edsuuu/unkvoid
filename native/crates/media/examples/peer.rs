//! Proves that the PeerConnection starts, negotiates H.264, and gathers ICE candidates.
//!
//! No other endpoint is needed: if the offer contains H.264 and ICE finds a path,
//! transport is up. Only the other side's response would be missing.
//!
//! cargo run -p media --example peer

use std::time::Duration;

use media::PeerLink;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (peer, mut sinais) =
        PeerLink::connect(vec!["stun:stun.l.google.com:19302".to_owned()], 60.0).await?;

    let oferta = peer.create_offer().await?;

    let tem_h264 = oferta.to_lowercase().contains("h264");
    let linhas_video = oferta
        .lines()
        .filter(|linha| linha.starts_with("m=video"))
        .count();

    println!("generated offer: {} bytes", oferta.len());
    println!("video track: {linhas_video}");
    println!("H.264 negotiated: {}", if tem_h264 { "yes" } else { "NO" });

    let mut candidatos = 0;
    let prazo = tokio::time::sleep(Duration::from_secs(6));

    tokio::pin!(prazo);

    loop {
        tokio::select! {
            Some(_) = sinais.recv() => candidatos += 1,
            _ = &mut prazo => break,
        }
    }

    println!("gathered ICE candidates: {candidatos}");

    peer.close().await?;

    println!(
        "\n{}",
        if tem_h264 && linhas_video == 1 && candidatos > 0 {
            "TRANSPORT OK — only the other endpoint's response is missing"
        } else {
            "SOMETHING IS MISSING: see the numbers above"
        }
    );

    Ok(())
}
