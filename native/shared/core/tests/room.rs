//! A sala inteira do núcleo (`Room`) contra um SFU de mentira, com WebSocket de verdade. O SFU
//! de mentira responde o que o `sfu/` responde e anota tudo o que recebe.
//!
//! ```text
//! cd native && cargo test -p core-app --test room
//! cd native && cargo test -p core-app --test room -- --ignored   # os que pedem GStreamer
//! ```

use std::net::UdpSocket;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use core_app::Identity;
use core_app::models::RoomIdentity;
use core_app::room::Room;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

mod common;

/// O que fazer depois de responder: nada, ou derrubar o socket logo depois de a câmera subir, e
/// na volta responder como entrada nova (a carência do servidor tinha expirado).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Stay,
    DropAfterCamera,
}

/// O que o SFU de mentira viu: cada pedido, na ordem, e a porta UDP de cada producer que abriu.
#[derive(Default)]
struct Seen {
    requests: Vec<Value>,
    ports: Vec<(String, Arc<UdpSocket>)>,
}

/// Quem está na sala é a Ana, com uma tela no ar; o `consumePlain` demora 300 ms para responder,
/// como uma ida e volta até os EUA e o mediasoup criando o consumer.
async fn fake_sfu(script: Script) -> (String, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let port = listener.local_addr().expect("address").port();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let numbers = Arc::new(AtomicUsize::new(0));
    let connections = Arc::new(AtomicUsize::new(0));
    let dead_port = UdpSocket::bind("127.0.0.1:0").expect("udp").local_addr().expect("address").port();

    tokio::spawn({
        let seen = Arc::clone(&seen);

        async move {
            while let Ok((stream, _)) = listener.accept().await {
                let seen = Arc::clone(&seen);
                let numbers = Arc::clone(&numbers);
                let first = connections.fetch_add(1, Ordering::SeqCst) == 0;

                tokio::spawn(async move {
                    let socket = tokio_tungstenite::accept_async(stream).await.expect("handshake");
                    let (mut sink, mut stream) = socket.split();
                    let (outgoing, mut to_send) = mpsc::unbounded_channel::<Message>();
                    // Derruba o socket sem fechamento, como o cabo puxado.
                    let cut = Arc::new(tokio::sync::Notify::new());

                    tokio::spawn(async move {
                        while let Some(message) = to_send.recv().await {
                            if sink.send(message).await.is_err() {
                                return;
                            }
                        }
                    });

                    loop {
                        let raw = tokio::select! {
                            () = cut.notified() => return,
                            next = stream.next() => match next {
                                Some(Ok(Message::Text(raw))) => raw,
                                _ => return,
                            },
                        };
                        let request: Value = serde_json::from_str(&raw).expect("json");
                        let action = request["action"].as_str().unwrap_or_default().to_owned();
                        let id = request["id"].clone();

                        seen.lock().expect("seen").requests.push(request.clone());

                        let reply = |data: Value| Message::text(json!({ "id": id, "ok": true, "data": data }).to_string());

                        match action.as_str() {
                            "join" => {
                                let _ = outgoing.send(reply(json!({
                                    "peerId": "me", "userId": "user:1", "e2eeSessionId": "test-room-session", "name": "Eu", "resumeKey": "k", "resumed": false,
                                    "can": ["speak", "stream", "video"], "elapsedMs": 0,
                                    "peers": [{ "peerId": "ana", "name": "Ana", "producers": [
                                        { "producerId": "tela-ana", "kind": "video", "source": "screen", "paused": false }
                                    ] }],
                                })));
                            }
                            "producePlain" => {
                                let number = numbers.fetch_add(1, Ordering::SeqCst);
                                let source = request["data"]["source"].as_str().unwrap_or_default().to_owned();
                                let udp = Arc::new(UdpSocket::bind("127.0.0.1:0").expect("udp"));
                                let port = udp.local_addr().expect("address").port();

                                seen.lock().expect("seen").ports.push((source.clone(), udp));

                                let _ = outgoing.send(reply(json!({
                                    "producerId": format!("{source}-{number}"), "ip": "127.0.0.1", "port": port,
                                    "srtpParameters": { "cryptoSuite": "AES_CM_128_HMAC_SHA1_80",
                                        "keyBase64": base64::engine::general_purpose::STANDARD.encode([9_u8; 30]) },
                                })));

                                if script == Script::DropAfterCamera && first && source == "camera" {
                                    let cut = Arc::clone(&cut);

                                    tokio::spawn(async move {
                                        tokio::time::sleep(Duration::from_millis(1_500)).await;
                                        cut.notify_one();
                                    });
                                }
                            }
                            "consumePlain" => {
                                let number = numbers.fetch_add(1, Ordering::SeqCst);
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

fn account() -> Identity {
    let token = common::join_token("01jabcdefghjkmnpqrstvwxyz0", "user:1", "Eu");

    Arc::new(move || {
        let token = token.clone();
        Box::pin(async move { Ok(RoomIdentity::Account { token }) })
    })
}

fn asked(seen: &Mutex<Seen>, action: &str) -> Vec<Value> {
    seen.lock().expect("seen").requests.iter().filter(|request| request["action"] == action).cloned().collect()
}

/// Um producer sem MemberId MLS autenticado é da geração antiga ou uma tentativa de downgrade:
/// nem `settle` nem o clique em "Assistir" podem abrir um consumer em claro.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_clear_producer_is_never_consumed_without_an_mls_member() {
    let (url, seen) = fake_sfu(Script::Stay).await;
    let (updates, _ui) = std::sync::mpsc::channel();
    let (room, _media) = Room::enter(&url, "01jabcdefghjkmnpqrstvwxyz0", account(), updates).await.expect("entrou");

    // Mesmo pedido duas vezes, o producer não autenticado não abre RTP em claro.
    room.watch(None).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(asked(&seen, "consumePlain").is_empty(), "producer sem MemberId MLS abriu consumer em claro");
    room.leave().await;
}

/// Um producer legado sem membro MLS nunca chega à etapa de `consumePlain`, ainda que a tela
/// seja fechada enquanto a entrada da sala está terminando.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_screen_closed_while_it_opens_stays_closed() {
    let (url, seen) = fake_sfu(Script::Stay).await;
    let (updates, _ui) = std::sync::mpsc::channel();
    let entering = tokio::spawn(async move { Room::enter(&url, "01jabcdefghjkmnpqrstvwxyz0", account(), updates).await.expect("entrou") });
    let deadline = Instant::now() + Duration::from_secs(3);

    while asked(&seen, "consumePlain").is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let (room, _media) = entering.await.expect("entrou");

    room.close_watched("tela-ana").await;
    tokio::time::sleep(Duration::from_millis(600)).await;

    let consumers: Vec<String> = asked(&seen, "resumeConsumer").iter().filter_map(|request| request["data"]["consumerId"].as_str().map(str::to_owned)).collect();
    let closed: Vec<String> = asked(&seen, "closeConsumer").iter().filter_map(|request| request["data"]["consumerId"].as_str().map(str::to_owned)).collect();

    assert!(consumers.is_empty() && closed.is_empty(), "um consumer sem chave MLS foi aberto: retomados {consumers:?}, fechados {closed:?}");
    assert_eq!(room.tiles()["tiles"].as_array().map(Vec::len), Some(0), "a tela fechada voltou a ser assistida: {}", room.tiles());
}

/// A queda longa (o servidor perdeu esta pessoa e ela volta como entrada nova) sobe de novo a
/// tela e o microfone; a câmera era a única que ficava para trás: parava, e quem estava na sala
/// deixava de ver a pessoa até ela religar à mão (auditoria P1-7). E a volta avisa a interface
/// uma vez só: o microfone voltava antes da câmera e o `room.mine` do meio dizia "câmera
/// desligada" — o app do macOS fechava a câmera que manda os quadros.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do GStreamer com o videotestsrc e o x264enc (o `UNKVOID_CAMERA_SOURCE` do build de depuração)"]
async fn the_camera_comes_back_after_a_long_drop() {
    // SAFETY: lido pela captura da câmera, que só este teste abre neste processo.
    unsafe { std::env::set_var("UNKVOID_CAMERA_SOURCE", "videotestsrc is-live=true pattern=ball") };

    let (url, seen) = fake_sfu(Script::DropAfterCamera).await;
    let (updates, ui) = std::sync::mpsc::channel::<String>();
    let (room, _media) = Room::enter(&url, "01jabcdefghjkmnpqrstvwxyz0", account(), updates).await.expect("entrou");
    let camera = capture::cameras().first().cloned().expect("a câmera de teste");
    let config = capture::CaptureConfig { source: capture::CaptureSource::Camera(camera.index), capture_audio: false, ..capture::CaptureConfig::default() };

    room.open_microphone(core_app::VoiceFilterConfig::default()).await.expect("o microfone abriu");
    room.open_captured_camera(config).await.expect("a câmera abriu");

    while ui.try_recv().is_ok() {}

    let deadline = Instant::now() + Duration::from_secs(20);
    let cameras = || asked(&seen, "producePlain").iter().filter(|request| request["data"]["source"] == "camera").count();

    while cameras() < 2 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(cameras(), 2, "a câmera não subiu de novo depois da queda: {:?}", asked(&seen, "producePlain"));
    assert_eq!(asked(&seen, "producePlain").iter().filter(|request| request["data"]["source"] == "mic").count(), 2, "o microfone não subiu de novo");
    assert_eq!(room.mine()["camera"], true, "a sala mostra a câmera desligada: {}", room.mine());

    let mine: Vec<Value> = ui
        .try_iter()
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .filter(|update| update["event"] == "room.mine")
        .map(|update| update["data"].clone())
        .collect();

    assert!(mine.iter().all(|data| data["camera"] == true && data["mic"] == true), "a volta avisou a câmera ou o microfone desligados no meio: {mine:?}");
    assert!(!mine.is_empty(), "a volta não avisou a interface");

    let again = seen.lock().expect("seen").ports.iter().rev().find(|(source, _)| source == "camera").map(|(_, socket)| Arc::clone(socket)).expect("porta");
    let mut datagram = [0_u8; 2_048];

    again.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");

    assert!(again.recv(&mut datagram).is_ok(), "nada da câmera chegou pelo caminho novo");

    room.leave().await;
}

/// Sair da sala fecha o socket: o SFU só tira a pessoa da sala no `leave`, e a sessão seguia
/// pingando de 5 em 5 s para sempre, com o `room.ping` da sala velha chegando à interface — uma
/// sala zumbi a cada troca de canal de voz (auditoria P2-4).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn leaving_the_room_closes_its_socket_and_stops_talking_to_the_ui() {
    let (url, seen) = fake_sfu(Script::Stay).await;
    let (updates, ui) = std::sync::mpsc::channel::<String>();
    let (room, media) = Room::enter(&url, "01jabcdefghjkmnpqrstvwxyz0", account(), updates).await.expect("entrou");

    room.leave().await;
    drop(room);
    drop(media);

    while ui.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_secs(6)).await;

    let requests: Vec<String> = seen.lock().expect("seen").requests.iter().map(|request| request["action"].as_str().unwrap_or_default().to_owned()).collect();
    let after: Vec<&String> = requests.iter().skip_while(|action| *action != "leave").skip(1).collect();
    let told: Vec<String> = ui.try_iter().filter(|line| line.contains("room.ping")).collect();

    assert!(after.is_empty(), "a sala que saiu continuou falando com o SFU: {after:?}");
    assert!(told.is_empty(), "a sala que saiu continuou falando com a interface: {told:?}");
}
