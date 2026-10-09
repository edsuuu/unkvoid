//! Auditoria de transmitir e assistir (`docs/auditoria-transmissao.md`): cada teste daqui
//! reproduz um defeito achado na revisão, pela API pública do `media` e com pacote de verdade.
//!
//! Os que reproduzem defeito afirmam o comportamento CERTO e estão marcados com `#[ignore]`,
//! para não deixar o `cargo test` vermelho enquanto ninguém corrige. Rodar:
//!
//! ```text
//! cd native && cargo test -p media --test audit -- --ignored --nocapture   # os que reproduzem (falham)
//! cd native && cargo test -p media --test audit                            # os descartados (passam)
//! ```
//!
//! Quando a correção entrar, o teste passa a passar: tire o `#[ignore]` e ele vira guarda.

use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use bytes::Bytes;
use media::{AudioEncoder, AudioUnpacker, EncodedFrame, PlainSender, Source};
use rtc::rtp::header::Header;
use rtc::rtp::packet::Packet;
use rtc::shared::marshal::Marshal;
use rtc::srtp::context::Context as SrtpContext;
use rtc::srtp::protection_profile::ProtectionProfile;

const BASE: u32 = 0x2000_0000;

/// Um bloco de 20 ms de voz em Opus, o que o microfone de quem fala manda a cada pacote.
fn voice_packet(encoder: &mut AudioEncoder, sequence: u16) -> Vec<u8> {
    let tone: Vec<f32> = (0..1_920).map(|index| (index as f32 * 0.05).sin() * 0.4).collect();
    let block = capture::AudioChunk { sample_rate: 48_000, channels: 2, samples: tone };
    let opus = encoder.push(&block).expect("encode").pop().expect("um pacote por bloco");
    let header = Header {
        version: 2,
        payload_type: 111,
        sequence_number: sequence,
        timestamp: u32::from(sequence).wrapping_mul(960),
        ssrc: 7,
        ..Header::default()
    };

    Packet { header, payload: Bytes::from(opus) }.marshal().expect("marshal").to_vec()
}

/// P0 — `unpack.rs:143-150`. Ensurdecer, ou deixar o som de uma tela mudo (ele chega mudo por
/// regra), faz o `PlainReceiver` parar de repassar os pacotes daquela rota (`receiver.rs:336`).
/// O servidor continua numerando, e na volta o `AudioUnpacker` compara o número novo com o
/// último que viu ANTES do mudo. Passados de 32 767 a 65 535 pacotes (de 10,9 a 21,8 min de som
/// a 50 pacotes por segundo), a diferença cai na metade de cima dos 16 bits e o pacote é tomado
/// por "atrasado": volta `None` e o `last` não anda. Todos os seguintes também, até a conta dar
/// a volta — minutos de silêncio depois de desensurdecer.
#[test]
#[ignore = "auditoria P0: reproduz o som que some por minutos depois de um mudo longo"]
fn audio_comes_back_right_after_a_long_local_mute() {
    let mut encoder = AudioEncoder::for_voice(48_000).expect("encoder");
    let mut unpacker = AudioUnpacker::new().expect("decoder");

    assert!(unpacker.push(&voice_packet(&mut encoder, 0)).is_some());
    assert!(unpacker.push(&voice_packet(&mut encoder, 1)).is_some());

    // 15 minutos ensurdecido: 45 000 pacotes que o receptor não repassou.
    let resumed_at: u16 = 1 + 45_000;
    let mut silent = 0_u32;

    for offset in 0..30_000_u16 {
        if unpacker.push(&voice_packet(&mut encoder, resumed_at.wrapping_add(offset))).is_some() {
            break;
        }

        silent += 1;
    }

    assert_eq!(
        silent,
        0,
        "o som só voltou depois de {silent} pacotes ({:.0} s de silêncio) — o mudo durou 15 min",
        f64::from(silent) * 0.02
    );
}

/// O mesmo, com um mudo curto: aqui o desenho funciona, e é por isso que o defeito não aparece
/// em teste rápido. Fica como contraprova.
#[test]
fn audio_comes_back_after_a_short_local_mute() {
    let mut encoder = AudioEncoder::for_voice(48_000).expect("encoder");
    let mut unpacker = AudioUnpacker::new().expect("decoder");

    assert!(unpacker.push(&voice_packet(&mut encoder, 0)).is_some());
    assert!(unpacker.push(&voice_packet(&mut encoder, 1 + 3_000)).is_some(), "um minuto de mudo volta na hora");
}

fn listener() -> (UdpSocket, SocketAddr) {
    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind");
    let address = socket.local_addr().expect("address");

    socket.set_read_timeout(Some(Duration::from_millis(500))).expect("timeout");

    (socket, address)
}

fn context(key: &[u8; 30]) -> SrtpContext {
    SrtpContext::new(&key[..16], &key[16..], ProtectionProfile::Aes128CmHmacSha1_80, None, None).expect("srtp")
}

fn frame(nal: u8, size: usize, timestamp_ns: u64) -> EncodedFrame {
    let mut data = vec![0, 0, 0, 1, nal];

    data.extend(std::iter::repeat_n(0xAB, size));

    EncodedFrame { data, keyframe: nal == 0x65, timestamp_ns }
}

/// O próximo RTP (não RTCP) que chegou ao "servidor": número de sequência, SSRC e quem mandou.
fn next_media(socket: &UdpSocket) -> (u16, u32, SocketAddr) {
    let mut buffer = [0_u8; 2_048];

    loop {
        let (size, from) = socket.recv_from(&mut buffer).expect("o pacote não chegou");

        if size >= 12 && !(200..=207).contains(&buffer[1]) {
            return (
                u16::from_be_bytes([buffer[2], buffer[3]]),
                u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
                from,
            );
        }
    }
}

fn drain(socket: &UdpSocket) {
    let mut buffer = [0_u8; 2_048];

    socket.set_read_timeout(Some(Duration::from_millis(50))).expect("timeout");
    while socket.recv(&mut buffer).is_ok() {}
    socket.set_read_timeout(Some(Duration::from_millis(500))).expect("timeout");
}

fn nack(media_ssrc: u32, sequence: u16) -> Vec<u8> {
    let mut packet = vec![0x81, 205, 0x00, 0x03, 0, 0, 0, 1];

    packet.extend_from_slice(&media_ssrc.to_be_bytes());
    packet.extend_from_slice(&sequence.to_be_bytes());
    packet.extend_from_slice(&[0, 0]);

    packet
}

/// P2 — `plain.rs:187,499-507`. Tela e câmera sobem pelo MESMO `PlainSender`, e o histórico de
/// reenvio é um só, indexado só pelo número de sequência. Cada origem numera por conta própria
/// (sequenciador sorteado por SSRC), e a tela anda dez vezes mais rápido que a câmera: de tempos
/// em tempos as duas faixas se cruzam dentro das 1 024 últimas entradas. Um NACK da tela, nessa
/// hora, acha primeiro o pacote da câmera com o mesmo número e reenvia o pacote errado — o
/// buraco da tela continua aberto e vira pedido de quadro-chave.
#[test]
#[ignore = "auditoria P2: reproduz o reenvio do pacote da câmera no lugar do da tela"]
fn a_nack_for_the_screen_resends_the_screen_packet_and_not_the_camera_one() {
    let (server, address) = listener();
    let server_key = PlainSender::generate_key();
    let mut sender = PlainSender::connect(address, &PlainSender::generate_key(), Some(&server_key), BASE).expect("connect");

    // Sem ritmo: o teste empurra dezenas de milhares de pacotes para alinhar as numerações.
    sender.follow_bitrate(4_000_000_000);

    let mut clock = 0_u64;
    let mut tick = || {
        clock += 16_666_667;
        clock
    };

    assert_eq!(sender.send_frame(Source::Screen, frame(0x65, 10, tick()), 60.0).expect("screen"), 1);
    let (screen_first, _, _) = next_media(&server);
    assert_eq!(sender.send_frame(Source::Camera, frame(0x65, 10, tick()), 30.0).expect("camera"), 1);
    let (camera_first, _, sender_address) = next_media(&server);

    // A próxima da câmera será `camera_first + 1`. Leva a tela até dez números antes dela.
    let target = camera_first.wrapping_add(1);
    let mut remaining = target.wrapping_sub(10).wrapping_sub(screen_first.wrapping_add(1));
    let bulk = sender.send_frame(Source::Screen, frame(0x41, 50 * 1_186, tick()), 60.0).expect("bulk") as u16;

    // O próprio quadro de calibragem andou a numeração.
    remaining = remaining.wrapping_sub(bulk);

    while remaining >= bulk {
        let sent = sender.send_frame(Source::Screen, frame(0x41, 50 * 1_186, tick()), 60.0).expect("bulk") as u16;

        assert_eq!(sent, bulk);
        remaining -= sent;
    }

    while remaining > 0 {
        sender.send_frame(Source::Screen, frame(0x41, 10, tick()), 60.0).expect("single");
        remaining -= 1;
    }

    // A câmera manda o pacote `target`; depois a tela passa pelo mesmo número.
    sender.send_frame(Source::Camera, frame(0x41, 10, tick()), 30.0).expect("camera");

    for _ in 0..11 {
        sender.send_frame(Source::Screen, frame(0x41, 10, tick()), 60.0).expect("single");
    }

    std::thread::sleep(Duration::from_millis(300));
    drain(&server);

    let mut server_srtp = context(&server_key);
    let protected = server_srtp.encrypt_rtcp(&nack(Source::Screen.ssrc(BASE), target)).expect("srtcp");

    server.send_to(&protected, sender_address).expect("nack");
    std::thread::sleep(Duration::from_millis(50));
    sender.read_feedback();

    let (sequence, ssrc, _) = next_media(&server);

    assert_eq!(sequence, target);
    assert_eq!(
        ssrc,
        Source::Screen.ssrc(BASE),
        "o NACK pediu o pacote {target} da TELA e voltou o da câmera (SSRC {ssrc:#x})"
    );
}

/// P2 — `plain.rs:495`, `sharing.rs:916-931` e `sharing.rs:1386-1393`. O pedido de quadro-chave
/// é um `bool` só para o remetente inteiro: não diz de que SSRC veio, e quem lê primeiro leva.
/// Com tela e câmera no ar, a thread da tela pode comer o PLI da câmera (e o contrário). Hoje
/// não morde porque o macOS e o Linux ignoram o pedido (`request_keyframe` vazio, GOP de 1 s) e
/// o Windows ainda não tem câmera; morde no dia em que qualquer um dos dois mudar.
#[test]
#[ignore = "auditoria P2: reproduz o PLI de uma origem consumido pela outra"]
fn a_keyframe_request_reaches_the_source_it_names() {
    let (server, address) = listener();
    let server_key = PlainSender::generate_key();
    let mut sender = PlainSender::connect(address, &PlainSender::generate_key(), Some(&server_key), BASE).expect("connect");

    sender.send_frame(Source::Screen, frame(0x65, 10, 1), 60.0).expect("screen");
    let (_, _, sender_address) = next_media(&server);
    sender.send_frame(Source::Camera, frame(0x65, 10, 2), 30.0).expect("camera");

    let mut pli = vec![0x81, 206, 0x00, 0x02, 0, 0, 0, 1];

    pli.extend_from_slice(&Source::Camera.ssrc(BASE).to_be_bytes());

    let protected = context(&server_key).encrypt_rtcp(&pli).expect("srtcp");

    server.send_to(&protected, sender_address).expect("pli");
    std::thread::sleep(Duration::from_millis(50));

    // A thread da tela lê primeiro (60 fps contra 30), como em `sharing.rs:916`.
    let read_by_the_screen = sender.read_feedback();
    let read_by_the_camera = sender.read_feedback();

    assert!(
        read_by_the_camera.keyframe,
        "o PLI da câmera foi entregue a quem lia pela tela (tela: {read_by_the_screen:?}, câmera: {read_by_the_camera:?})"
    );
}

/// Descartado: o SRTP do `rtc` atravessa a volta do número de sequência (65 535 → 0), também com
/// pacote fora de ordem bem na volta e com a numeração começando perto do fim. Era a suspeita de
/// a tela morrer depois de ~65 mil pacotes (~1 min a 1080p60).
#[test]
fn srtp_survives_the_sequence_wrap_with_reordering() {
    let key = PlainSender::generate_key();

    for start in [65_500_u16, 30_000, 32_760, 0] {
        let mut sending = context(&key);
        let mut receiving = context(&key);
        let packets: Vec<(u16, Vec<u8>)> = (0..70_000_u32)
            .map(|index| {
                let sequence = start.wrapping_add(index as u16);
                let header = Header { version: 2, payload_type: 96, sequence_number: sequence, timestamp: index, ssrc: 9, ..Header::default() };
                let plain = Packet { header, payload: Bytes::from(vec![index as u8; 40]) }.marshal().expect("marshal");

                (sequence, sending.encrypt_rtp(&plain).expect("encrypt").to_vec())
            })
            .collect();
        let mut order: Vec<usize> = (0..packets.len()).collect();

        // Troca de lugar cada par que atravessa uma volta, como a rede faria.
        let wraps: Vec<usize> = packets.iter().enumerate().skip(1).filter(|(_, (sequence, _))| *sequence == 0).map(|(index, _)| index).collect();

        for index in wraps {
            order.swap(index - 1, index);
        }

        for index in order {
            receiving
                .decrypt_rtp(&packets[index].1)
                .unwrap_or_else(|error| panic!("pacote {} (início {start}) não abriu: {error}", packets[index].0));
        }
    }
}
