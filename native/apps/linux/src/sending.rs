//! O teste vivo da captura do Linux: a tela, o encoder e o socket, sem sala.
//!
//! Subir a tela para o SFU é do `core_app::room::Room`, o mesmo que o macOS e o Windows usam
//! — o remetente próprio que morava aqui foi para lá. Fica a prova de que a captura e o
//! encoder desta máquina vivem, porque ela só roda com uma tela de verdade na frente.

mod tests {
    use capture::{CaptureConfig, CaptureSource};
    use core_app::sharing;
    use media::Source;

    /// Prova que a captura e o encoder do Linux abrem, comprimem e que o quadro sai pelo
    /// socket.
    ///
    /// `#[ignore]` porque precisa de uma tela de verdade — roda com
    /// `UNKVOID_CAPTURE=x11 cargo test -p unkvoid-linux -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn the_screen_capture_leaves_through_the_socket() {
        // Alguém tem de estar escutando: no Linux um socket UDP ligado a uma porta fechada
        // recebe o ICMP de volta e falha no envio seguinte, o que contaria como erro nosso.
        let listening = std::net::UdpSocket::bind("127.0.0.1:41999").expect("a porta abriu");
        let session = sharing::Session::default();

        session.use_sfu("127.0.0.1:41999", None).expect("o remetente abriu");

        let config = CaptureConfig {
            source: CaptureSource::PrimaryDisplay,
            capture_audio: false,
            quality: capture::Quality::Hd720,
            frame_rate: 30,
            ..CaptureConfig::default()
        };

        let mut broadcast = session.start(config, Some(Source::Screen), None).expect("a captura abriu");

        std::thread::sleep(std::time::Duration::from_secs(3));

        let stats = broadcast.stats();

        println!("{stats}");
        let _ = broadcast.stop();
        drop(listening);

        assert!(stats["sent"].as_u64().unwrap_or(0) > 0, "nada saiu pelo socket em 3 s: {stats}");
        assert_eq!(stats["sendErrors"].as_u64().unwrap_or(1), 0, "o envio deu erro: {stats}");
    }
}
