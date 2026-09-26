//! Os testes vivos da captura do Windows: a tela, o encoder da placa e o socket, sem sala.
//!
//! Subir a tela para o SFU é do `core_app::room::Room`, o mesmo que o macOS usa — o aperto de
//! mão do `producePlain` que morava aqui foi para lá. Ficam as provas de que a captura e o
//! encoder desta máquina vivem, porque elas só rodam com uma tela de verdade na frente.

mod tests {
    use capture::{CaptureConfig, CaptureSource};
    use core_app::sharing;
    use media::Source;

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
