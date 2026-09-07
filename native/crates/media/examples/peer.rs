//! Prova que a PeerConnection sobe, negocia H.264 e reúne candidatos ICE.
//!
//! Não precisa de outra ponta: se a oferta sai com H.264 e o ICE encontra caminho,
//! o transporte está de pé. Faltaria só o outro lado responder.
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

    println!("oferta gerada: {} bytes", oferta.len());
    println!("trilha de vídeo: {linhas_video}");
    println!("H.264 negociado: {}", if tem_h264 { "sim" } else { "NÃO" });

    let mut candidatos = 0;
    let prazo = tokio::time::sleep(Duration::from_secs(6));

    tokio::pin!(prazo);

    loop {
        tokio::select! {
            Some(_) = sinais.recv() => candidatos += 1,
            _ = &mut prazo => break,
        }
    }

    println!("candidatos ICE reunidos: {candidatos}");

    peer.close().await?;

    println!(
        "\n{}",
        if tem_h264 && linhas_video == 1 && candidatos > 0 {
            "TRANSPORTE OK — falta só a outra ponta responder"
        } else {
            "ALGO FALTOU: veja os números acima"
        }
    );

    Ok(())
}
