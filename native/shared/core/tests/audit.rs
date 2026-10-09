//! Auditoria de transmitir e assistir (`docs/auditoria-transmissao.md`): a sala inteira do
//! núcleo (`Room`) contra um SFU de mentira, com WebSocket de verdade. O SFU de mentira responde
//! o que o `sfu/` responde (as linhas citadas em cada teste) e anota tudo o que recebe.
//!
//! Os que reproduzem defeito afirmam o comportamento CERTO e estão com `#[ignore]`. Rodar:
//!
//! ```text
//! cd native && cargo test -p core-app --test audit -- --ignored --nocapture   # reproduzem (falham)
//! cd native && cargo test -p core-app --test audit                            # guardas (passam)
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use core_app::Identity;
use core_app::models::RoomIdentity;
use core_app::room::Room;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

/// O que o SFU de mentira viu: cada pedido, na ordem, e os pings que chegaram depois do `leave`.
#[derive(Default)]
struct Seen {
    requests: Vec<Value>,
    pings_after_leave: usize,
    left: bool,
}

/// O que fazer depois de responder o `join`: nada, ou mover a pessoa como o `kickUser` com
/// destino (`sfu/src/Services/Room.ts:266-267`: `moved` e o fechamento com 4003).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Stay,
    MoveAfterJoin,
}

/// Um SFU de mentira. Quem está na sala é a Ana, com uma tela no ar; o `consumePlain` demora
/// 300 ms para responder, como uma ida e volta até os EUA e o mediasoup criando o consumer.
async fn fake_sfu(script: Script) -> (String, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let port = listener.local_addr().expect("address").port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let consumers = Arc::new(AtomicUsize::new(0));
    let dead_port = std::net::UdpSocket::bind("127.0.0.1:0").expect("udp").local_addr().expect("address").port();

    tokio::spawn({
        let seen = Arc::clone(&seen);

        async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let seen = Arc::clone(&seen);
                let consumers = Arc::clone(&consumers);

                tokio::spawn(async move {
                    let socket = tokio_tungstenite::accept_async(stream).await.expect("handshake");
                    let (mut sink, mut stream) = socket.split();
                    let (outgoing, mut to_send) = mpsc::unbounded_channel::<Message>();

                    tokio::spawn(async move {
                        while let Some(message) = to_send.recv().await {
                            let closing = matches!(message, Message::Close(_));

                            if sink.send(message).await.is_err() || closing {
                                return;
                            }
                        }
                    });

                    while let Some(Ok(Message::Text(raw))) = stream.next().await {
                        let request: Value = serde_json::from_str(&raw).expect("json");
                        let action = request["action"].as_str().unwrap_or_default().to_owned();
                        let id = request["id"].clone();

                        {
                            let mut seen = seen.lock().expect("seen");

                            if action == "ping" && seen.left {
                                seen.pings_after_leave += 1;
                            }

                            seen.left |= action == "leave";
                            seen.requests.push(request.clone());
                        }

                        let reply = |data: Value| Message::text(json!({ "id": id, "ok": true, "data": data }).to_string());

                        match action.as_str() {
                            "join" => {
                                let _ = outgoing.send(reply(json!({
                                    "peerId": "me", "name": "Eu", "resumeKey": "k", "resumed": false,
                                    "can": ["speak", "stream", "video"], "elapsedMs": 0,
                                    "peers": [{ "peerId": "ana", "name": "Ana", "producers": [
                                        { "producerId": "tela-ana", "kind": "video", "source": "screen", "paused": false }
                                    ] }],
                                })));

                                if script == Script::MoveAfterJoin {
                                    tokio::time::sleep(Duration::from_millis(500)).await;
                                    let _ = outgoing.send(Message::text(
                                        json!({ "event": "moved", "data": { "to": "01kdestinodestinodestinodd", "by": "Mod" } }).to_string(),
                                    ));
                                    let _ = outgoing.send(Message::Close(Some(CloseFrame { code: CloseCode::Library(4003), reason: "moved".into() })));
                                }
                            }
                            "consumePlain" => {
                                let number = consumers.fetch_add(1, Ordering::SeqCst);
                                let outgoing = outgoing.clone();
                                let producer = request["data"]["producerId"].clone();

                                tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_millis(300)).await;
                                    let _ = outgoing.send(Message::text(
                                        json!({ "id": id, "ok": true, "data": {
                                            "consumerId": format!("consumer-{number}"), "producerId": producer,
                                            "kind": "video", "source": "screen", "ip": "127.0.0.1", "port": dead_port,
                                            "srtpParameters": { "cryptoSuite": "AES_CM_128_HMAC_SHA1_80",
                                                "keyBase64": base64::engine::general_purpose::STANDARD.encode([7_u8; 30]) },
                                            "payloadType": 96, "ssrc": 1_000 + number, "rtx": null, "receiving": true,
                                        } })
                                        .to_string(),
                                    ));
                                });
                            }
                            "leave" => {
                                let _ = outgoing.send(reply(json!({ "status": "left" })));
                            }
                            _ => {
                                let _ = outgoing.send(reply(json!({})));
                            }
                        }
                    }
                });
            }
        }
    });

    (format!("ws://127.0.0.1:{port}"), seen)
}

/// A identidade de visitante, contando quantas vezes o núcleo pediu uma (uma por entrada).
fn guest(asked: Arc<AtomicUsize>) -> Identity {
    Arc::new(move || {
        asked.fetch_add(1, Ordering::SeqCst);

        Box::pin(async {
            Ok(RoomIdentity::Guest { room: "auditoria01".into(), name: "Eu".into(), install_id: "audit".into() })
        })
    })
}

fn consumed_per_producer(seen: &Mutex<Seen>) -> HashMap<String, usize> {
    let mut count = HashMap::new();

    for request in &seen.lock().expect("seen").requests {
        if request["action"] == "consumePlain" {
            *count.entry(request["data"]["producerId"].as_str().unwrap_or_default().to_owned()).or_insert(0) += 1;
        }
    }

    count
}

/// P1 — `room.rs:369-417` e `room.rs:421-494`. O `consume` olha se já assiste
/// (`is_watching`), espera o `consumePlain` e só depois marca. Duas chamadas de `consume_all` ao
/// mesmo tempo — o `settle` da entrada (`run`), o `newProducer`, o `watch` do botão "Assistir",
/// o vigia do caminho de chegada (`guard_watching`) — passam as duas pela pergunta e pedem DOIS
/// consumers do mesmo producer. O segundo `Watching::start` não faz nada (`watching.rs:151`),
/// mas o id dele sobrescreve o do primeiro em `consumers`: o consumer que de fato chega fica
/// órfão — pausar, fechar e "janela fora da vista" passam a agir no outro, a banda dele não
/// para nunca, e quem transmite vê esta pessoa em dobro em `watchers`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "auditoria P1: reproduz dois consumers do mesmo producer por corrida"]
async fn one_producer_is_consumed_once_even_when_asked_twice_at_once() {
    let (url, seen) = fake_sfu(Script::Stay).await;
    let (updates, _ui) = std::sync::mpsc::channel();
    let (room, _media) = Room::enter(&url, "auditoria01", guest(Arc::default()), updates).await.expect("entrou");

    // O "Assistir" clicado enquanto a entrada ainda assiste o que já estava no ar.
    room.watch(None).await;
    tokio::time::sleep(Duration::from_millis(800)).await;

    // A pessoa fecha a tela: tudo o que foi retomado para ela tem de fechar.
    room.close_watched("tela-ana").await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let count = consumed_per_producer(&seen);
    let ids = |action: &str| -> Vec<String> {
        seen.lock()
            .expect("seen")
            .requests
            .iter()
            .filter(|request| request["action"] == action)
            .filter_map(|request| request["data"]["consumerId"].as_str().map(str::to_owned))
            .collect()
    };
    let (resumed, closed) = (ids("resumeConsumer"), ids("closeConsumer"));
    let orphans: Vec<&String> = resumed.iter().filter(|consumer| !closed.contains(consumer)).collect();

    assert!(orphans.is_empty(), "consumer retomado e nunca fechado: {orphans:?} (consumePlain por producer: {count:?}, retomados {resumed:?}, fechados {closed:?})");
    assert_eq!(count.get("tela-ana"), Some(&1), "consumePlain por producer: {count:?}");
}

/// P2 — `room.rs:1011-1023` e `session.rs:429-434`. Sair da sala manda `leave` e só: ninguém
/// fecha o socket, e o SFU também não (`sfu/src/Http/Controller/LeaveController.ts` só tira a
/// pessoa da sala). O `supervise` da sessão continua pingando a cada 5 s, a fila de eventos não
/// fecha, o laço `run` segura o `Arc<Room>` para sempre e os dois vigias seguem acordando. Cada
/// troca de canal de voz deixa uma sala zumbi: um WebSocket aberto, três tarefas, e `room.ping`
/// chegando na interface junto com o da sala nova.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "auditoria P2: reproduz a sala que não morre depois do leave (dura ~11 s)"]
async fn leaving_the_room_closes_its_socket_and_stops_talking_to_the_ui() {
    let (url, seen) = fake_sfu(Script::Stay).await;
    let (updates, ui) = std::sync::mpsc::channel::<String>();
    let (room, media) = Room::enter(&url, "auditoria01", guest(Arc::default()), updates).await.expect("entrou");

    room.leave().await;
    drop(room);
    drop(media);

    while ui.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_secs(11)).await;

    let after: Vec<String> = ui.try_iter().filter(|line| line.contains("room.ping")).collect();
    let pings = seen.lock().expect("seen").pings_after_leave;

    assert_eq!(pings, 0, "a sala que saiu continuou pingando o SFU ({pings} pings em 11 s)");
    assert!(after.is_empty(), "a sala que saiu continuou falando com a interface: {after:?}");
}

/// Descartado, contra o SFU de VERDADE: a volta do número de sequência de 16 bits atravessa o
/// caminho inteiro — o SRTP do `rtc` cifrando aqui, o libsrtp do mediasoup abrindo lá, o
/// mediasoup cifrando para quem assiste e o `rtc` abrindo de novo no `PlainReceiver`. Manda
/// 140 mil pacotes (as duas numerações, a do producer e a do consumer, dão a volta pelo menos
/// uma vez) e confere que os quadros continuam chegando inteiros no fim.
///
/// Precisa do SFU no ar (o `SFU_SECRET` qualquer, de 32+ caracteres):
///
/// ```text
/// cd sfu && pnpm run build && SFU_SECRET=$(printf 'a%.0s' {1..40}) SFU_CONNECTIONS_PER_MINUTE=1000 node dist/server.js
/// cd native && UNKVOID_AUDIT_SFU=ws://127.0.0.1:3000/sfu cargo test -p core-app --test audit real_sfu -- --ignored --nocapture --test-threads=1
/// ```
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "auditoria: precisa do SFU no ar (UNKVOID_AUDIT_SFU); passa — a volta do seq foi descartada"]
async fn the_sequence_wrap_crosses_the_real_sfu_both_ways() {
    use media::{EncodedFrame, PlainReceiver, PlainSender, Rtx, Source, Stream, VideoUnpacker};

    let url = std::env::var("UNKVOID_AUDIT_SFU").expect("UNKVOID_AUDIT_SFU=ws://…/sfu");
    let room = format!("auditwrap{}", rand_suffix());
    let join = |name: &str| json!({ "room": room, "name": name, "installId": format!("audit-{name}") });
    let (publisher, _publisher_events) = core_app::SfuClient::connect(&url).await.expect("publisher");
    let (watcher, _watcher_events) = core_app::SfuClient::connect(&url).await.expect("watcher");

    publisher.call("join", join("publisher")).await.expect("join publisher");
    watcher.call("join", join("watcher")).await.expect("join watcher");

    let encode = |key: &[u8]| base64::engine::general_purpose::STANDARD.encode(key);
    let decode = |value: &Value| base64::engine::general_purpose::STANDARD.decode(value.as_str().expect("key")).expect("base64");
    let base = PlainSender::random_ssrc_base();
    let key = PlainSender::generate_key();
    let produced = publisher
        .call("producePlain", json!({
            "kind": "video", "source": "screen",
            "rtpParameters": PlainSender::rtp_parameters(Source::Screen, base),
            "srtpParameters": { "cryptoSuite": PlainSender::CRYPTO_SUITE, "keyBase64": encode(&key) },
        }))
        .await
        .expect("producePlain");
    let address = format!("{}:{}", produced["ip"].as_str().expect("ip"), produced["port"]);
    let mut sender = PlainSender::connect(address.as_str(), &key, Some(&decode(&produced["srtpParameters"]["keyBase64"])), base).expect("sender");

    sender.follow_bitrate(400_000_000);

    let watch_key = PlainSender::generate_key();
    let consumed = watcher
        .call("consumePlain", json!({
            "producerId": produced["producerId"],
            "srtpParameters": { "cryptoSuite": PlainSender::CRYPTO_SUITE, "keyBase64": encode(&watch_key) },
        }))
        .await
        .expect("consumePlain");
    let decoder = std::net::UdpSocket::bind("127.0.0.1:0").expect("decoder");
    let receiver = PlainReceiver::start(
        format!("{}:{}", consumed["ip"].as_str().expect("ip"), consumed["port"]).as_str(),
        &watch_key,
        &decode(&consumed["srtpParameters"]["keyBase64"]),
    )
    .expect("receiver");

    media::grow_receive_buffer(&decoder);
    decoder.set_read_timeout(Some(Duration::from_millis(200))).expect("timeout");
    receiver.route(Stream {
        id: "screen".into(),
        payload_type: u8::try_from(consumed["payloadType"].as_u64().expect("pt")).expect("pt"),
        to: decoder.local_addr().expect("address"),
        ssrc: consumed["ssrc"].as_u64().map(|ssrc| ssrc as u32),
        video: true,
        rtx: consumed["rtx"]["ssrc"].as_u64().map(|ssrc| Rtx { ssrc: ssrc as u32, payload_type: consumed["rtx"]["payloadType"].as_u64().unwrap_or(0) as u8 }),
    });
    watcher.call("resumeConsumer", json!({ "consumerId": consumed["consumerId"] })).await.expect("resume");

    let frames = Arc::new(Mutex::new(Vec::<std::time::Instant>::new()));
    let reading = std::thread::spawn({
        let frames = Arc::clone(&frames);

        move || {
            let mut unpacker = VideoUnpacker::default();
            let mut datagram = [0_u8; 1_500];
            let mut quiet = 0;

            while quiet < 15 {
                match decoder.recv(&mut datagram) {
                    Ok(size) => {
                        quiet = 0;

                        if unpacker.push(&datagram[..size]).is_some() {
                            frames.lock().expect("frames").push(std::time::Instant::now());
                        }
                    }
                    Err(_) => quiet += 1,
                }
            }
        }
    });

    // Quadros de ~20 pacotes; quadro-chave quando o servidor pede e a cada 120.
    let keyframe = |size: usize| {
        let mut data = vec![0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0xDA, 0x01, 0x40, 0x16, 0xEC, 0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, 0, 0, 0, 1, 0x65];
        data.extend(std::iter::repeat_n(0x11, size));
        data
    };
    let delta = |size: usize| {
        let mut data = vec![0, 0, 0, 1, 0x41];
        data.extend(std::iter::repeat_n(0x22, size));
        data
    };
    let (mut sent_packets, mut index, mut clock) = (0_usize, 0_u64, 0_u64);
    let started = std::time::Instant::now();

    while sent_packets < 140_000 {
        let asked = sender.read_feedback().keyframe;
        let data = if asked || index % 120 == 0 { keyframe(23_000) } else { delta(23_000) };

        clock += 16_666_667;
        sent_packets += sender.send_frame(Source::Screen, EncodedFrame { keyframe: data[4] == 0x67, data, timestamp_ns: clock }, 60.0).expect("send");
        index += 1;

        if index % 4 == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    let finished = std::time::Instant::now();

    tokio::time::sleep(Duration::from_secs(1)).await;
    receiver.stop();
    reading.join().expect("reader");

    let frames = frames.lock().expect("frames");
    let last_stretch = finished - (finished - started) / 10;
    let late = frames.iter().filter(|at| **at >= last_stretch).count();
    let counters = receiver.counters("screen");

    println!("{index} quadros, {sent_packets} pacotes em {:?}; {} quadros inteiros chegaram, {late} no último décimo; {counters:?}", finished - started, frames.len());

    assert!(frames.len() as u64 > index * 8 / 10, "só {} de {index} quadros chegaram inteiros", frames.len());
    assert!(late as u64 > index / 10 * 7 / 10, "depois da volta do número de sequência os quadros pararam: {late} no último décimo");
}

/// Descartado, contra o SFU de verdade: com 3% do RTP jogado fora na chegada (`UNKVOID_LOSS`,
/// o mesmo botão do `receiver.rs`), o NACK do `PlainReceiver` volta pelo RTX do mediasoup e o
/// `unwrap_rtx` devolve o pacote original. Rodar como o teste de cima, trocando o nome:
/// `… --test audit lost_packets -- --ignored --nocapture`.
///
/// O que ele também mostra (medido em 09/10: `received 3960, recovered 99, lost 4`, 192 de 360
/// quadros inteiros): numa ida e volta curta os três pedidos do mesmo pacote saem de 40 em 40 ms,
/// e o mediasoup só reenvia um por janela de 100 ms (sem RR do receptor ele não sabe a ida e
/// volta). Quando esse único reenvio também se perde, o buraco é largado aos 250 ms e a imagem
/// para até o quadro-chave — ~0,7 s cada. Ver `sfu/audit/nack-repeat.mjs` e o P2 do relatório.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "auditoria: precisa do SFU no ar (UNKVOID_AUDIT_SFU); passa — NACK e RTX funcionam"]
async fn lost_packets_come_back_over_the_real_sfu_rtx() {
    use media::{EncodedFrame, PlainReceiver, PlainSender, Rtx, Source, Stream, VideoUnpacker};

    // SAFETY: lido uma vez pelo `PlainReceiver::start` logo abaixo, e apagado em seguida.
    unsafe { std::env::set_var("UNKVOID_LOSS", "3") };

    let url = std::env::var("UNKVOID_AUDIT_SFU").expect("UNKVOID_AUDIT_SFU=ws://…/sfu");
    let room = format!("auditloss{}", rand_suffix());
    let join = |name: &str| json!({ "room": room, "name": name, "installId": format!("audit-{name}") });
    let (publisher, _publisher_events) = core_app::SfuClient::connect(&url).await.expect("publisher");
    let (watcher, _watcher_events) = core_app::SfuClient::connect(&url).await.expect("watcher");

    publisher.call("join", join("publisher")).await.expect("join publisher");
    watcher.call("join", join("watcher")).await.expect("join watcher");

    let encode = |key: &[u8]| base64::engine::general_purpose::STANDARD.encode(key);
    let decode = |value: &Value| base64::engine::general_purpose::STANDARD.decode(value.as_str().expect("key")).expect("base64");
    let base = PlainSender::random_ssrc_base();
    let key = PlainSender::generate_key();
    let produced = publisher
        .call("producePlain", json!({
            "kind": "video", "source": "screen",
            "rtpParameters": PlainSender::rtp_parameters(Source::Screen, base),
            "srtpParameters": { "cryptoSuite": PlainSender::CRYPTO_SUITE, "keyBase64": encode(&key) },
        }))
        .await
        .expect("producePlain");
    let address = format!("{}:{}", produced["ip"].as_str().expect("ip"), produced["port"]);
    let mut sender = PlainSender::connect(address.as_str(), &key, Some(&decode(&produced["srtpParameters"]["keyBase64"])), base).expect("sender");
    let watch_key = PlainSender::generate_key();
    let consumed = watcher
        .call("consumePlain", json!({
            "producerId": produced["producerId"],
            "srtpParameters": { "cryptoSuite": PlainSender::CRYPTO_SUITE, "keyBase64": encode(&watch_key) },
        }))
        .await
        .expect("consumePlain");
    let decoder = std::net::UdpSocket::bind("127.0.0.1:0").expect("decoder");
    let receiver = PlainReceiver::start(
        format!("{}:{}", consumed["ip"].as_str().expect("ip"), consumed["port"]).as_str(),
        &watch_key,
        &decode(&consumed["srtpParameters"]["keyBase64"]),
    )
    .expect("receiver");

    // SAFETY: o mesmo teste, logo depois da única leitura; o da volta do seq não herda a perda.
    unsafe { std::env::remove_var("UNKVOID_LOSS") };

    assert!(consumed["rtx"]["ssrc"].is_u64(), "o consumePlain não anunciou RTX: {consumed}");
    decoder.set_read_timeout(Some(Duration::from_millis(200))).expect("timeout");
    receiver.route(Stream {
        id: "screen".into(),
        payload_type: u8::try_from(consumed["payloadType"].as_u64().expect("pt")).expect("pt"),
        to: decoder.local_addr().expect("address"),
        ssrc: consumed["ssrc"].as_u64().map(|ssrc| ssrc as u32),
        video: true,
        rtx: consumed["rtx"]["ssrc"].as_u64().map(|ssrc| Rtx { ssrc: ssrc as u32, payload_type: consumed["rtx"]["payloadType"].as_u64().unwrap_or(0) as u8 }),
    });
    watcher.call("resumeConsumer", json!({ "consumerId": consumed["consumerId"] })).await.expect("resume");

    let frames = Arc::new(AtomicUsize::new(0));
    let reading = std::thread::spawn({
        let frames = Arc::clone(&frames);

        move || {
            let mut unpacker = VideoUnpacker::default();
            let mut datagram = [0_u8; 1_500];
            let mut quiet = 0;

            while quiet < 15 {
                match decoder.recv(&mut datagram) {
                    Ok(size) => {
                        quiet = 0;

                        if unpacker.push(&datagram[..size]).is_some() {
                            frames.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    Err(_) => quiet += 1,
                }
            }
        }
    });
    let frame = |keyframe: bool| {
        let mut data = if keyframe {
            vec![0, 0, 0, 1, 0x67, 0x42, 0xE0, 0x1F, 0xDA, 0x01, 0x40, 0x16, 0xEC, 0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, 0, 0, 0, 1, 0x65]
        } else {
            vec![0, 0, 0, 1, 0x41]
        };
        data.extend(std::iter::repeat_n(0x33, 12_000));
        EncodedFrame { keyframe, data, timestamp_ns: 0 }
    };
    let mut keyframes = 0;

    // Sessenta quadros por segundo por seis segundos; quadro-chave só quando pedem.
    for index in 0..360_u64 {
        let asked = sender.read_feedback().keyframe || index == 0;
        let mut next = frame(asked);

        keyframes += usize::from(asked);
        next.timestamp_ns = index * 16_666_667;
        sender.send_frame(Source::Screen, next, 60.0).expect("send");
        tokio::time::sleep(Duration::from_millis(16)).await;
    }

    tokio::time::sleep(Duration::from_secs(1)).await;

    let counters = receiver.counters("screen").expect("contagem");

    receiver.stop();
    reading.join().expect("reader");

    let whole = frames.load(Ordering::SeqCst);

    println!("360 quadros, {keyframes} quadros-chave pedidos, {whole} inteiros; {counters:?}");

    assert!(counters.recovered > 0, "nada voltou pelo RTX: {counters:?}");
    assert!(whole > 0, "nenhum quadro inteiro: {counters:?}");
}

fn rand_suffix() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.subsec_nanos())
}

/// Guarda (passa na base de 09/10, depois do merge #43/#45): movida por um moderador, a sala
/// para, avisa a interface com o destino e NÃO volta sozinha para a origem — na base anterior
/// ela reconectava na origem com token novo (que o Laravel recusa por 60 s) até desistir.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_moved_room_tells_the_destination_and_does_not_rejoin_the_origin() {
    let (url, _seen) = fake_sfu(Script::MoveAfterJoin).await;
    let (updates, ui) = std::sync::mpsc::channel::<String>();
    let asked = Arc::new(AtomicUsize::new(0));
    let (_room, _media) = Room::enter(&url, "auditoria01", guest(Arc::clone(&asked)), updates).await.expect("entrou");

    tokio::time::sleep(Duration::from_secs(4)).await;

    let told: Vec<Value> = ui
        .try_iter()
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .filter(|event| event["event"] == "room.session")
        .collect();

    assert!(
        told.iter().any(|event| event["data"]["state"] == "moved" && event["data"]["to"] == "01kdestinodestinodestinodd"),
        "a interface não soube do destino: {told:?}"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1, "a sala movida pediu outra entrada na origem");
}
