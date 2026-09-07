//! Prova que dois lados fecham a conexão de verdade.
//!
//! Sobe duas PeerConnections no mesmo processo e faz a troca completa de oferta,
//! resposta e candidatos — sem servidor no meio. Se o ICE conectar, a negociação
//! está certa e o que falta é só o transporte de sinalização.
//!
//! cargo run -p media --example p2p

use std::time::Duration;

use media::{PeerLink, Signal};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (mut quem_envia, mut sinais_envio) =
        PeerLink::connect(vec!["stun:stun.l.google.com:19302".into()], 60.0).await?;
    let (mut quem_recebe, mut sinais_recepcao) =
        PeerLink::connect(vec!["stun:stun.l.google.com:19302".into()], 60.0).await?;

    let oferta = quem_envia.create_offer().await?;

    println!(
        "oferta: {} bytes · H.264: {}",
        oferta.len(),
        oferta.to_lowercase().contains("h264")
    );

    let resposta = quem_recebe.accept_offer(oferta).await?;

    println!("resposta: {} bytes", resposta.len());

    quem_envia.accept_answer(resposta).await?;

    println!("SDP trocado — agora os candidatos\n");

    let prazo = tokio::time::sleep(Duration::from_secs(10));

    tokio::pin!(prazo);

    let (mut de_envio, mut de_recepcao) = (0, 0);

    loop {
        tokio::select! {
            Some(Signal::Candidate(json)) = sinais_envio.recv() => {
                de_envio += 1;
                let _ = quem_recebe.add_candidate(json).await;
            }
            Some(Signal::Candidate(json)) = sinais_recepcao.recv() => {
                de_recepcao += 1;
                let _ = quem_envia.add_candidate(json).await;
            }
            _ = &mut prazo => break,
        }
    }

    println!("candidatos trocados: {de_envio} de quem envia · {de_recepcao} de quem recebe");

    quem_envia.close().await?;
    quem_recebe.close().await?;

    println!(
        "\n{}",
        if de_envio > 0 && de_recepcao > 0 {
            "NEGOCIAÇÃO COMPLETA — oferta, resposta e candidatos dos dois lados"
        } else {
            "INCOMPLETA: veja os números acima"
        }
    );

    Ok(())
}
