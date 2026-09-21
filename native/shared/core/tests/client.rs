//! O cliente contra um servidor de mentira, mas WebSocket de verdade.
//!
//! O que se prova aqui é o que quebra em produção e não aparece em teste de unidade: duas
//! chamadas em voo não trocarem de resposta, e evento não ser confundido com resposta.

use std::time::Duration;

use core_app::SfuClient;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Responde na ordem inversa da que recebeu, e manda um evento no meio. É o cenário que
/// pega correlação errada: quem espera pela ordem de chegada recebe a resposta do outro.
async fn server_out_of_order() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let port = listener.local_addr().expect("address").port();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = tokio_tungstenite::accept_async(stream).await.expect("handshake");
        let mut received: Vec<Value> = Vec::new();

        while let Some(Ok(message)) = socket.next().await {
            let Message::Text(raw) = message else {
                continue;
            };

            let request: Value = serde_json::from_str(&raw).expect("parse request");

            received.push(request);

            if received.len() < 2 {
                continue;
            }

            let event = json!({ "event": "peerJoined", "data": { "peerId": "xyz" } });

            socket.send(Message::text(event.to_string())).await.expect("send event");

            for request in received.iter().rev() {
                let reply = json!({
                    "id": request["id"],
                    "ok": true,
                    "data": { "echo": request["action"] },
                });

                socket.send(Message::text(reply.to_string())).await.expect("reply");
            }

            received.clear();
        }
    });

    format!("ws://127.0.0.1:{port}")
}

#[tokio::test]
async fn each_call_gets_its_own_reply() {
    let url = server_out_of_order().await;
    let (client, mut events) = SfuClient::connect(&url).await.expect("connect");

    let first = {
        let client = client.clone();

        tokio::spawn(async move { client.call("identify", json!({ "token": "a" })).await })
    };

    let second = {
        let client = client.clone();

        tokio::spawn(async move { client.call("subscribe", json!({ "channel": "channel.1" })).await })
    };

    let first = first.await.expect("task").expect("first call");
    let second = second.await.expect("task").expect("second call");

    // O servidor respondeu na ordem inversa de propósito.
    assert_eq!(first["echo"], "identify", "identify reply went to the other call");
    assert_eq!(second["echo"], "subscribe", "subscribe reply went to the other call");

    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("event in time")
        .expect("event");

    assert_eq!(event.name, "peerJoined");
    assert_eq!(event.data["peerId"], "xyz");
}

#[tokio::test]
async fn a_server_error_arrives_typed_with_its_status() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let port = listener.local_addr().expect("address").port();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = tokio_tungstenite::accept_async(stream).await.expect("handshake");

        while let Some(Ok(Message::Text(raw))) = socket.next().await {
            let request: Value = serde_json::from_str(&raw).expect("parse");
            let reply = json!({
                "id": request["id"],
                "ok": false,
                "status": 403,
                "error": "not authorized for this channel",
            });

            socket.send(Message::text(reply.to_string())).await.expect("reply");
        }
    });

    let (client, _events) = SfuClient::connect(&format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");

    let failure = client.subscribe("channel.999").await.expect_err("should have failed");
    let server_error = failure
        .downcast_ref::<core_app::ServerError>()
        .expect("server error with status");

    assert_eq!(server_error.status, 403);
    assert_eq!(server_error.action, "subscribe");
}

#[tokio::test]
async fn a_closed_socket_leaves_no_call_hanging() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let port = listener.local_addr().expect("address").port();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let socket = tokio_tungstenite::accept_async(stream).await.expect("handshake");

        // Fecha sem responder nada: é a queda no meio de uma chamada.
        drop(socket);
    });

    let (client, _events) = SfuClient::connect(&format!("ws://127.0.0.1:{port}"))
        .await
        .expect("connect");

    let answered = tokio::time::timeout(Duration::from_secs(3), client.identify("token")).await;

    assert!(answered.is_ok(), "the call hung after the socket dropped");
    assert!(answered.expect("in time").is_err(), "should have failed");
}

/// Um SFU que responde a tudo e **derruba o socket** depois do primeiro `join`. É a queda
/// de sinalização de 20/09/2026, em que o TCP sumiu com a mídia junto.
async fn sfu_that_drops_once() -> (String, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let port = listener.local_addr().expect("address").port();
    let seen: std::sync::Arc<std::sync::Mutex<Vec<Value>>> = Default::default();
    let recorded = seen.clone();

    tokio::spawn(async move {
        let mut connections = 0;

        while let Ok((stream, _)) = listener.accept().await {
            connections += 1;

            let mut socket = tokio_tungstenite::accept_async(stream).await.expect("handshake");

            while let Some(Ok(Message::Text(raw))) = socket.next().await {
                let request: Value = serde_json::from_str(&raw).expect("parse");

                recorded.lock().expect("record").push(request.clone());

                let reply = json!({
                    "id": request["id"],
                    "ok": true,
                    "data": {
                        "peerId": "mine",
                        "name": "Ada",
                        "resumeKey": "k",
                        "peers": [],
                        "can": ["speak"],
                    },
                });

                socket.send(Message::text(reply.to_string())).await.expect("reply");

                if connections == 1 && request["action"] == "join" {
                    break;
                }
            }
        }
    });

    (format!("ws://127.0.0.1:{port}"), seen)
}

#[tokio::test]
async fn a_dropped_socket_comes_back_with_the_resume_key_and_a_fresh_token() {
    let (url, seen) = sfu_that_drops_once().await;
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter = asked.clone();

    let identity: core_app::Identity = std::sync::Arc::new(move || {
        let counter = counter.clone();

        Box::pin(async move {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            Ok(core_app::models::RoomIdentity::Account { token: "fresco".into() })
        })
    });

    let (session, mut events) = core_app::Session::join(&url, "sala", identity).await.expect("join");

    let mut names = Vec::new();

    while names.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(20), events.recv())
            .await
            .expect("a volta demorou demais")
            .expect("a fila do app fechou");

        names.push(event.name);
    }

    assert_eq!(names, ["sessionLost", "sessionRejoined"]);

    let joins: Vec<Value> = seen
        .lock()
        .expect("record")
        .iter()
        .filter(|request| request["action"] == "join")
        .cloned()
        .collect();

    assert_eq!(joins.len(), 2, "não tentou voltar");
    assert_eq!(joins[1]["data"]["resume"], true);
    assert_eq!(joins[1]["data"]["resumeKey"], "k");

    // O token de voz vale 60 s: guardar o primeiro faria a volta levar um token vencido.
    assert_eq!(asked.load(std::sync::atomic::Ordering::Relaxed), 2, "reaproveitou o token velho");
    assert!(session.peers().iter().any(|peer| peer.self_peer), "a lista não foi refeita");

    let deadline = std::time::Instant::now() + Duration::from_secs(15);

    while std::time::Instant::now() < deadline {
        if seen.lock().expect("record").iter().any(|request| request["action"] == "ping") {
            return;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    panic!("o app nunca mandou o ping de 5 s do contrato");
}

/// Um servidor que aceita a conexão e nunca completa o aperto de mão do WebSocket. Sem
/// prazo, o cliente espera para sempre — e a tela que depende dele fica parada, que foi
/// exatamente o que pareceu estar acontecendo com o app.
#[tokio::test]
async fn connecting_to_a_silent_server_gives_up_instead_of_hanging_forever() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let port = listener.local_addr().expect("address").port();

    tokio::spawn(async move {
        let _kept = listener.accept().await.expect("accept");

        tokio::time::sleep(Duration::from_secs(120)).await;
    });

    let answered = tokio::time::timeout(
        Duration::from_secs(30),
        SfuClient::connect(&format!("ws://127.0.0.1:{port}")),
    )
    .await;

    assert!(answered.is_ok(), "ficou pendurado além do prazo do próprio cliente");
    assert!(answered.expect("in time").is_err(), "disse que conectou num servidor mudo");
}
