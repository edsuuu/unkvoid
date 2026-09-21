//! Publicar a tela no SFU.
//!
//! A captura, o encoder e o envio são do `core_app::sharing`, que é o mesmo código das
//! quatro interfaces — aqui só mora o aperto de mão que o servidor pede antes do primeiro
//! pacote: o `producePlain` leva o SSRC, o tipo de payload e a chave SRTP, e devolve para
//! onde mandar.
//!
//! Regra de negócio nenhuma: quem diz o que esta sessão pode produzir é o `can` do `join`.

use anyhow::Result;
use base64::Engine;
use capture::{CaptureConfig, CaptureSource};
use core_app::Session;
use core_app::protocol::action;
use core_app::sharing;
use media::Source;
use serde_json::{Value, json};

/// A tela sobe em 1080p60: é a qualidade que o app existe para entregar sem pesar no jogo.
const SCREEN_QUALITY: capture::Quality = capture::Quality::Hd1080;
const SCREEN_FRAME_RATE: u32 = 60;

/// Liga a captura da tela e a publica. Quem já está transmitindo não republica.
pub async fn share_screen(session: &Session, media: &sharing::ActiveSession) -> Result<()> {
    if media.0.lock().await.screen.is_some() {
        return Ok(());
    }

    let config = CaptureConfig {
        source: CaptureSource::PrimaryDisplay,
        capture_audio: true,
        quality: SCREEN_QUALITY,
        frame_rate: SCREEN_FRAME_RATE,
        ..CaptureConfig::default()
    };

    // No Wayland a tela é escolhida no seletor do sistema, que abre aqui. No Windows é uma
    // chamada vazia, mas mantê-la é o que faz este arquivo servir aos dois.
    let prepared = tokio::task::block_in_place(|| capture::prepare(&config))?;
    let mut held = media.0.lock().await;
    let mut last = Value::Null;

    for source in [Source::Screen, Source::ScreenAudio] {
        let mut request = json!({
            "kind": if source.is_video() { "video" } else { "audio" },
            "source": name_of(source),
        });

        merge(&mut request, held.sfu_offer(source));
        last = session.client().call(action::PRODUCE_PLAIN, request).await?;
    }

    let address = format!("{}:{}", text(&last, "ip"), last["port"]);
    let server_key = decode(&last["srtpParameters"]["keyBase64"]);

    held.use_sfu(&address, server_key)?;

    // Abrir captura e encoder bloqueia — no Windows são Graphics Capture e Media Foundation.
    let broadcast = tokio::task::block_in_place(|| {
        held.start(config, Some(Source::Screen), Some(Source::ScreenAudio))
    })?;

    // O `start` consumiu a escolha do seletor; largá-la antes fecharia o que ele abriu.
    drop(prepared);

    held.screen = Some(broadcast);

    Ok(())
}

/// Para de capturar e fecha o que o servidor ainda tem aberto desta pessoa.
pub async fn stop_screen(session: &Session, media: &sharing::ActiveSession) {
    let mut held = media.0.lock().await;

    if let Some(mut broadcast) = held.screen.take()
        && let Err(failure) = tokio::task::block_in_place(|| broadcast.stop())
    {
        tracing::warn!(%failure, "a captura não parou limpa");
    }

    held.release_if_idle();
    drop(held);

    // O servidor fecha os producers desta pessoa quando ela sai; aqui só se pede o fim da
    // tela, e quem sabe os identificadores é o `Roster` da sessão.
    for producer in session.peers().iter().filter(|peer| peer.self_peer).flat_map(|peer| &peer.producers) {
        if producer.source != "screen" && producer.source != "screenAudio" {
            continue;
        }

        if let Err(failure) =
            session.client().call(action::CLOSE_PRODUCER, json!({ "producerId": producer.producer_id })).await
        {
            tracing::warn!(%failure, producer = %producer.producer_id, "o producer não fechou no servidor");
        }
    }
}

/// O nome que o servidor conhece para cada origem. Errar aqui é o SFU recusar a oferta.
fn name_of(source: Source) -> &'static str {
    match source {
        Source::Screen => "screen",
        Source::ScreenAudio => "screenAudio",
        Source::Camera => "camera",
        Source::Mic => "mic",
    }
}

/// Junta a oferta ao pedido sem aninhar: o servidor espera os campos no primeiro nível.
fn merge(request: &mut Value, offer: Value) {
    let (Some(target), Some(fields)) = (request.as_object_mut(), offer.as_object()) else {
        return;
    };

    for (name, value) in fields {
        target.insert(name.clone(), value.clone());
    }
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_owned()
}

fn decode(value: &Value) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(value.as_str()?).ok()
}

/// O erro que a pessoa lê quando a captura não abre. O detalhe fica no log.
pub fn said(failure: &anyhow::Error) -> String {
    tracing::warn!(%failure, "a tela não subiu");

    "Não deu para compartilhar a tela.".to_owned()
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Prova que a captura e o encoder do Windows abrem e produzem quadro. Sem destino: o
    /// remetente é opcional, e o que se quer saber aqui é se o caminho até o encoder vive.
    ///
    /// `#[ignore]` porque precisa de uma tela de verdade — roda com
    /// `cargo test -p unkvoid-windows -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_screen_capture_produces_encoded_frames() {
        let session = sharing::Session::default();
        let config = CaptureConfig {
            source: CaptureSource::PrimaryDisplay,
            capture_audio: false,
            quality: capture::Quality::Hd720,
            frame_rate: 30,
            ..CaptureConfig::default()
        };

        let broadcast = session.start(config, Some(Source::Screen), None).expect("a captura abriu");

        std::thread::sleep(std::time::Duration::from_secs(3));

        let stats = broadcast.stats();

        println!("{stats}");

        assert!(broadcast.frames() > 0, "nenhum quadro saiu da captura em 3 s");
        assert!(stats["encoded"].as_u64().unwrap_or(0) > 0, "nada foi codificado: {stats}");
    }

    /// O mesmo caminho, agora com destino: prova que o quadro codificado vira pacote e sai
    /// pelo socket. O endereço não precisa de ninguém escutando — o que se mede aqui é o
    /// envio, não a entrega.
    #[test]
    #[ignore]
    fn the_encoded_frames_leave_through_the_socket() {
        let session = sharing::Session::default();

        session.use_sfu("127.0.0.1:41999", None).expect("o remetente abriu");

        let config = CaptureConfig {
            source: CaptureSource::PrimaryDisplay,
            capture_audio: false,
            quality: capture::Quality::Hd720,
            frame_rate: 30,
            ..CaptureConfig::default()
        };

        let broadcast = session.start(config, Some(Source::Screen), None).expect("a captura abriu");

        std::thread::sleep(std::time::Duration::from_secs(3));

        let stats = broadcast.stats();

        println!("{stats}");

        assert!(stats["sent"].as_u64().unwrap_or(0) > 0, "nada saiu pelo socket: {stats}");
        assert_eq!(stats["sendErrors"].as_u64().unwrap_or(1), 0, "o envio deu erro: {stats}");
    }
}
