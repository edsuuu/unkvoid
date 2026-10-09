//! A sala inteira do núcleo (`Room`) contra um SFU de mentira, com WebSocket de verdade. O SFU
//! de mentira responde o que o `sfu/` responde e anota tudo o que recebe.
//!
//! ```text
//! cd native && cargo test -p core-app --test room
//! cd native && cargo test -p core-app --test room -- --ignored   # os que pedem GStreamer
//! ```

use std::collections::HashMap;
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
                                    "peerId": "me", "name": "Eu", "resumeKey": "k", "resumed": false,
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

fn guest() -> Identity {
    Arc::new(|| Box::pin(async { Ok(RoomIdentity::Guest { room: "salateste01".into(), name: "Eu".into(), install_id: "teste".into() }) }))
}

fn asked(seen: &Mutex<Seen>, action: &str) -> Vec<Value> {
    seen.lock().expect("seen").requests.iter().filter(|request| request["action"] == action).cloned().collect()
}

/// O `settle` da entrada, o `newProducer`, o "Assistir" e o vigia do caminho de chegada chamam
/// `consume_all` por tarefas diferentes. Duas ao mesmo tempo pediam dois `consumePlain` do mesmo
/// producer; o id do segundo sobrescrevia o do primeiro, e o consumer que de fato chegava ficava
/// órfão: pausar e fechar agiam no outro, e a banda dele não parava nunca (auditoria P1-2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_producer_is_consumed_once_even_when_asked_twice_at_once() {
    let (url, seen) = fake_sfu(Script::Stay).await;
    let (updates, _ui) = std::sync::mpsc::channel();
    let (room, _media) = Room::enter(&url, "salateste01", guest(), updates).await.expect("entrou");

    // O "Assistir" clicado enquanto a entrada ainda abre o que já estava no ar.
    room.watch(None).await;
    tokio::time::sleep(Duration::from_millis(800)).await;

    // A pessoa fecha a tela: tudo o que foi retomado para ela tem de fechar.
    room.close_watched("tela-ana").await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut count: HashMap<String, usize> = HashMap::new();

    for request in asked(&seen, "consumePlain") {
        *count.entry(request["data"]["producerId"].as_str().unwrap_or_default().to_owned()).or_default() += 1;
    }

    let ids = |action: &str| -> Vec<String> { asked(&seen, action).iter().filter_map(|request| request["data"]["consumerId"].as_str().map(str::to_owned)).collect() };
    let (resumed, closed) = (ids("resumeConsumer"), ids("closeConsumer"));
    let orphans: Vec<&String> = resumed.iter().filter(|consumer| !closed.contains(consumer)).collect();

    assert!(orphans.is_empty(), "consumer retomado e nunca fechado: {orphans:?} (consumePlain por producer: {count:?}, retomados {resumed:?}, fechados {closed:?})");
    assert_eq!(count.get("tela-ana"), Some(&1), "consumePlain por producer: {count:?}");
}

/// Fechar a tela enquanto o `consumePlain` dela está no ar: o consumer que chega depois fecha,
/// em vez de retomar uma tela que a pessoa acabou de fechar.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_screen_closed_while_it_opens_stays_closed() {
    let (url, seen) = fake_sfu(Script::Stay).await;
    let (updates, _ui) = std::sync::mpsc::channel();
    let entering = tokio::spawn(async move { Room::enter(&url, "salateste01", guest(), updates).await.expect("entrou") });
    let deadline = Instant::now() + Duration::from_secs(3);

    while asked(&seen, "consumePlain").is_empty() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let (room, _media) = entering.await.expect("entrou");

    room.close_watched("tela-ana").await;
    tokio::time::sleep(Duration::from_millis(600)).await;

    let consumers: Vec<String> = asked(&seen, "resumeConsumer").iter().filter_map(|request| request["data"]["consumerId"].as_str().map(str::to_owned)).collect();
    let closed: Vec<String> = asked(&seen, "closeConsumer").iter().filter_map(|request| request["data"]["consumerId"].as_str().map(str::to_owned)).collect();

    assert!(consumers.iter().all(|consumer| closed.contains(consumer)), "retomados {consumers:?}, fechados {closed:?}");
    assert_eq!(room.tiles()["tiles"].as_array().map(Vec::len), Some(0), "a tela fechada voltou a ser assistida: {}", room.tiles());
}

/// A queda longa (o servidor perdeu esta pessoa e ela volta como entrada nova) sobe de novo a
/// tela e o microfone; a câmera era a única que ficava para trás: parava, e quem estava na sala
/// deixava de ver a pessoa até ela religar à mão (auditoria P1-7).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do GStreamer com o videotestsrc e o x264enc (o `UNKVOID_CAMERA_SOURCE` do build de depuração)"]
async fn the_camera_comes_back_after_a_long_drop() {
    // SAFETY: lido pela captura da câmera, que só este teste abre neste processo.
    unsafe { std::env::set_var("UNKVOID_CAMERA_SOURCE", "videotestsrc is-live=true pattern=ball") };

    let (url, seen) = fake_sfu(Script::DropAfterCamera).await;
    let (updates, _ui) = std::sync::mpsc::channel();
    let (room, _media) = Room::enter(&url, "salateste01", guest(), updates).await.expect("entrou");
    let camera = capture::cameras().first().cloned().expect("a câmera de teste");
    let config = capture::CaptureConfig { source: capture::CaptureSource::Camera(camera.index), capture_audio: false, ..capture::CaptureConfig::default() };

    room.open_captured_camera(config).await.expect("a câmera abriu");

    let deadline = Instant::now() + Duration::from_secs(20);
    let cameras = || asked(&seen, "producePlain").iter().filter(|request| request["data"]["source"] == "camera").count();

    while cameras() < 2 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert_eq!(cameras(), 2, "a câmera não subiu de novo depois da queda: {:?}", asked(&seen, "producePlain"));
    assert_eq!(room.mine()["camera"], true, "a sala mostra a câmera desligada: {}", room.mine());

    let again = seen.lock().expect("seen").ports.iter().rev().find(|(source, _)| source == "camera").map(|(_, socket)| Arc::clone(socket)).expect("porta");
    let mut datagram = [0_u8; 2_048];

    again.set_read_timeout(Some(Duration::from_secs(5))).expect("timeout");

    assert!(again.recv(&mut datagram).is_ok(), "nada da câmera chegou pelo caminho novo");

    room.leave().await;
}
