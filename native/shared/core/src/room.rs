//! A sala viva, para a interface que não é Rust: o socket, o que sobe e o que chega, atrás
//! de uma fila de avisos em JSON.
//!
//! É o que o `bridge.rs` do Linux faz com as mãos dele, escrito uma vez para quem vem pela
//! ABI. Regra nenhuma mora aqui: o que esta sessão pode mandar é o `can` do `join`, que é
//! do servidor; a interface só esconde botão.
//!
//! Os avisos que saem (`{"event", "data"}`):
//! `room.peers` a lista inteira, `room.tiles` o que dá para assistir, `room.mine` o que
//! esta pessoa manda e pode mandar, `room.session` (`lost`, `rejoined`, `gone`, e `replaced`
//! ou `kicked` quando o servidor tirou esta sessão de propósito),
//! `room.failed` (`watch`, `share`, `mic`), `room.watchers` quem assiste a cada tela,
//! `room.ping` a ida e volta até o servidor e
//! `room.level` o nível do microfone.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use anyhow::{Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use capture::CaptureConfig;
use media::Source;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::models::ProducerInfo;
use crate::protocol::{Event, action, local};
use crate::session::{Identity, Session};
use crate::sharing::{self, AudioFeed};
use crate::watching::{Incoming, Media, Watching};

/// A única suíte combinada com o servidor, dos dois lados.
const CRYPTO_SUITE: &str = "AES_CM_128_HMAC_SHA1_80";

pub struct Room {
    session: Arc<Session>,
    sending: Mutex<sharing::Session>,
    /// O que o servidor abriu para cada origem que sobe, para fechar na saída.
    producers: Mutex<HashMap<Source, Vec<String>>>,
    microphone: Mutex<Option<Arc<AudioFeed>>>,
    /// Como o microfone abre. Guardado aqui para valer também no microfone que abrir depois.
    input_mode: Mutex<sharing::InputMode>,
    #[cfg(target_os = "macos")]
    camera: Mutex<Option<Arc<sharing::VideoFeed>>>,
    watching: Mutex<Watching>,
    /// O consumer que o servidor abriu para cada producer assistido, para pausar e fechar.
    consumers: Mutex<HashMap<String, String>>,
    /// O que a pessoa fechou de propósito: não reabre sozinho, só pelo "Assistir".
    closed: Mutex<HashSet<String>>,
    paused: Mutex<HashSet<String>>,
    /// "Ver o que a sala vê": assistir à própria tela, que custa um decodificador a mais.
    self_view: std::sync::atomic::AtomicBool,
    /// A receita da tela que está subindo, para republicar depois de uma queda longa.
    shared: Mutex<Option<CaptureConfig>>,
    /// Os cartões do último aviso: a interface só redesenha o palco quando eles mudam.
    shown: Mutex<Value>,
    updates: Sender<String>,
}

impl Room {
    /// Entra na sala e passa a cuidar dela. Devolve também a fila do que chega para assistir.
    pub async fn enter(
        url: &str,
        room: &str,
        identity: Identity,
        updates: Sender<String>,
    ) -> Result<(Arc<Self>, Receiver<Media>)> {
        let (session, events) = Session::join(url, room, identity).await?;
        let (watching, media) = Watching::new();

        let room = Arc::new(Self {
            session,
            sending: Mutex::default(),
            producers: Mutex::default(),
            microphone: Mutex::default(),
            input_mode: Mutex::default(),
            #[cfg(target_os = "macos")]
            camera: Mutex::default(),
            watching: Mutex::new(watching),
            consumers: Mutex::default(),
            closed: Mutex::default(),
            paused: Mutex::default(),
            self_view: std::sync::atomic::AtomicBool::new(false),
            shared: Mutex::default(),
            shown: Mutex::default(),
            updates,
        });

        tokio::spawn({
            let room = Arc::clone(&room);

            async move { room.run(events).await }
        });

        Ok((room, media))
    }

    async fn run(self: Arc<Self>, mut events: UnboundedReceiver<Event>) {
        self.announce_peers();
        self.settle().await;

        // O socket fechado encerra a fila, e é aí que este laço termina.
        while let Some(event) = events.recv().await {
            let changed = self.session.apply(&event);

            match event.name.as_str() {
                "newProducer" => self.consume_all().await,
                "producerClosed" => {
                    let producer_id = event.data["producerId"].as_str().unwrap_or_default();

                    lock(&self.watching).stop(Some(producer_id));
                    lock(&self.consumers).remove(producer_id);
                    lock(&self.closed).remove(producer_id);
                    lock(&self.paused).remove(producer_id);
                }
                local::SESSION_LOST => self.tell("room.session", json!({ "state": "lost" })),
                local::SESSION_REJOINED => {
                    self.tell("room.session", json!({ "state": "rejoined" }));

                    // Entrada nova (a carência do servidor expirou) perdeu tudo o que
                    // estava aberto lá: o que ficou aqui só atrapalha.
                    if !self.session.resumed() {
                        self.republish().await;
                    }

                    self.settle().await;
                }
                local::SESSION_GONE => self.tell("room.session", json!({ "state": "gone" })),
                // A sala acabou para esta sessão, e não volta: o que subia para, e a interface
                // tira a pessoa de lá com o motivo.
                "replaced" | "kicked" => {
                    self.stop_everything();
                    self.tell("room.session", json!({ "state": event.name }));
                }
                local::PING_MEASURED => self.tell("room.ping", json!({ "ms": event.data })),
                // Quem está assistindo a cada tela: o servidor já manda a lista pronta.
                "watchers" => self.tell("room.watchers", event.data.clone()),
                _ => {}
            }

            if changed {
                self.announce_peers();
            }

            self.announce_tiles();
        }
    }

    /// O que fazer assim que a sala abre, e de novo a cada volta.
    async fn settle(&self) {
        self.consume_all().await;
        self.announce_tiles();
        self.announce_mine();
    }

    /// Assiste a tudo o que ainda não está sendo assistido. Idempotente de propósito: o
    /// producer novo e a volta depois de uma queda chamam a mesma coisa.
    async fn consume_all(&self) {
        let peers = self.session.peers();
        let mine = self.self_view.load(std::sync::atomic::Ordering::Relaxed);
        let closed = lock(&self.closed).clone();

        // Juntado antes do laço: um iterador com fechamento não atravessa um `await`.
        let wanted: Vec<ProducerInfo> = peers
            .iter()
            .flat_map(|peer| {
                peer.producers
                    .iter()
                    .map(move |producer| (peer.self_peer, producer))
            })
            .filter(|(own, producer)| {
                if *own {
                    mine && producer.source == "screen"
                } else {
                    !closed.contains(&producer.producer_id)
                }
            })
            .map(|(_, producer)| producer.clone())
            .collect();

        for producer in &wanted {
            if let Err(failure) = self.consume(producer).await {
                tracing::warn!(%failure, producer = %producer.producer_id, "não deu para assistir");
                self.tell("room.failed", json!({ "what": "watch" }));
            }
        }
    }

    /// O `consumePlain` devolve por onde a transmissão vem e a chave para abri-la; o
    /// `resumeConsumer` é o que solta o primeiro pacote.
    async fn consume(&self, producer: &ProducerInfo) -> Result<()> {
        if lock(&self.watching).is_watching(&producer.producer_id) {
            return Ok(());
        }

        let key = lock(&self.watching).key();
        let answer = self
            .session
            .client()
            .call(
                action::CONSUME_PLAIN,
                json!({
                    "producerId": producer.producer_id,
                    "srtpParameters": { "cryptoSuite": CRYPTO_SUITE, "keyBase64": STANDARD.encode(key) },
                }),
            )
            .await?;

        let consumer_id = text(&answer, "consumerId");
        let address = address_of(&answer);
        let server_key = decode(&answer["srtpParameters"]["keyBase64"])
            .ok_or_else(|| anyhow!("consumidor sem chave"))?;
        let kind = answer["kind"].as_str().unwrap_or(&producer.kind).to_owned();
        let source = answer["source"]
            .as_str()
            .unwrap_or(&producer.source)
            .to_owned();

        let started = lock(&self.watching).start(Incoming {
            producer_id: producer.producer_id.clone(),
            kind: &kind,
            address: &address,
            server_key: &server_key,
            payload_type: answer["payloadType"].as_u64().unwrap_or_default() as u8,
            ssrc: answer["ssrc"].as_u64().map(|ssrc| ssrc as u32),
            always_muted: source == "screenAudio",
        });

        if let Err(failure) = started {
            let _ = self
                .session
                .client()
                .call(action::CLOSE_CONSUMER, json!({ "consumerId": consumer_id }))
                .await;

            return Err(failure);
        }

        self.session
            .client()
            .call(
                action::RESUME_CONSUMER,
                json!({ "consumerId": consumer_id }),
            )
            .await?;
        lock(&self.consumers).insert(producer.producer_id.clone(), consumer_id);

        Ok(())
    }

    /// Para de receber uma transmissão sem sair da sala; ela continua ao vivo para os outros.
    pub async fn close_watched(&self, producer_id: &str) {
        lock(&self.watching).stop(Some(producer_id));
        lock(&self.closed).insert(producer_id.to_owned());
        lock(&self.paused).remove(producer_id);

        let consumer = lock(&self.consumers).remove(producer_id);

        if let Some(consumer_id) = consumer {
            let _ = self
                .session
                .client()
                .call(action::CLOSE_CONSUMER, json!({ "consumerId": consumer_id }))
                .await;
        }

        self.announce_tiles();
    }

    /// O "Assistir" de uma transmissão que a pessoa tinha fechado, ou de todas (`None`).
    pub async fn watch(&self, producer_id: Option<&str>) {
        match producer_id {
            Some(producer_id) => {
                lock(&self.closed).remove(producer_id);
            }
            None => lock(&self.closed).clear(),
        }

        self.consume_all().await;
        self.announce_tiles();
    }

    /// Pausar é o servidor parar de mandar: a banda é devolvida, e a volta pede um keyframe.
    pub async fn pause_watched(&self, producer_id: &str, paused: bool) {
        let Some(consumer_id) = lock(&self.consumers).get(producer_id).cloned() else {
            return;
        };

        let acted = if paused {
            action::PAUSE_CONSUMER
        } else {
            action::RESUME_CONSUMER
        };

        if self
            .session
            .client()
            .call(acted, json!({ "consumerId": consumer_id }))
            .await
            .is_ok()
        {
            if paused {
                lock(&self.paused).insert(producer_id.to_owned());
            } else {
                lock(&self.paused).remove(producer_id);
            }
        }

        self.announce_tiles();
    }

    /// "Ver o que a sala vê": liga ou desliga assistir à própria tela.
    pub async fn set_self_view(&self, wanted: bool) {
        self.self_view
            .store(wanted, std::sync::atomic::Ordering::Relaxed);

        if wanted {
            self.consume_all().await;
        } else {
            let own: Vec<String> = self
                .session
                .peers()
                .iter()
                .filter(|peer| peer.self_peer)
                .flat_map(|peer| {
                    peer.producers
                        .iter()
                        .map(|producer| producer.producer_id.clone())
                })
                .collect();

            for producer_id in own {
                lock(&self.watching).stop(Some(&producer_id));

                let consumer = lock(&self.consumers).remove(&producer_id);

                if let Some(consumer_id) = consumer {
                    let _ = self
                        .session
                        .client()
                        .call(action::CLOSE_CONSUMER, json!({ "consumerId": consumer_id }))
                        .await;
                }
            }
        }

        self.announce_tiles();
        self.announce_mine();
    }

    /// Troca resolução e quadros por segundo com a transmissão no ar, sem fechar o producer.
    pub async fn change_quality(&self, quality: capture::Quality, frame_rate: u32) -> Result<()> {
        let changed = tokio::task::block_in_place(|| match lock(&self.sending).screen.as_mut() {
            Some(broadcast) => broadcast.restart(quality, frame_rate),
            None => Ok(()),
        });

        if let Some(config) = lock(&self.shared).as_mut() {
            config.quality = quality;
            config.frame_rate = frame_rate;
        }

        changed
    }

    /// Compartilhar a tela. Abre a origem no servidor e só então captura: sem o
    /// `producePlain` não há porta para onde mandar, e o quadro sairia no vazio.
    pub async fn share(&self, config: CaptureConfig) -> Result<()> {
        if lock(&self.sending).screen.is_some() {
            return Ok(());
        }

        let audio = config.capture_audio.then_some(Source::ScreenAudio);
        let producers = self.open(&[Some(Source::Screen), audio]).await?;
        let recipe = config.clone();

        let started = tokio::task::block_in_place(|| {
            let mut sending = lock(&self.sending);
            let broadcast = sending.start(config, Some(Source::Screen), audio)?;

            sending.screen = Some(broadcast);

            anyhow::Ok(())
        });

        if let Err(failure) = started {
            self.close(producers).await;

            return Err(failure);
        }

        lock(&self.producers).insert(Source::Screen, producers);
        *lock(&self.shared) = Some(recipe);
        self.announce_mine();

        Ok(())
    }

    pub async fn stop_sharing(&self) {
        *lock(&self.shared) = None;

        if self
            .self_view
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.set_self_view(false).await;
        }

        let broadcast = lock(&self.sending).screen.take();

        if let Some(mut broadcast) = broadcast {
            let _ = tokio::task::block_in_place(|| broadcast.stop());
        }

        self.retire(Source::Screen).await;
    }

    /// Abre o microfone no servidor. Quem captura é a interface, e o som entra por `speak`.
    pub async fn open_microphone(&self) -> Result<()> {
        if lock(&self.microphone).is_some() {
            return Ok(());
        }

        self.reopen_microphone().await
    }

    async fn reopen_microphone(&self) -> Result<()> {
        let producers = self.open(&[Some(Source::Mic)]).await?;
        let opened = lock(&self.sending).feed(Source::Mic);

        let feed = match opened {
            Ok(feed) => Arc::new(feed),
            Err(failure) => {
                self.close(producers).await;

                return Err(failure);
            }
        };

        let updates = self.updates.clone();

        feed.set_input_mode(*lock(&self.input_mode));
        feed.on_level(move |level| {
            let percent = sharing::level_percent(&[level]);
            let _ = updates.send(json!({ "event": "room.level", "channel": null, "data": { "level": level, "percent": percent } }).to_string());
        });

        *lock(&self.microphone) = Some(feed);
        lock(&self.producers).insert(Source::Mic, producers);
        self.announce_mine();

        Ok(())
    }

    pub async fn close_microphone(&self) {
        *lock(&self.microphone) = None;

        self.retire(Source::Mic).await;
    }

    pub fn set_input_mode(&self, mode: sharing::InputMode) {
        *lock(&self.input_mode) = mode;

        if let Some(feed) = lock(&self.microphone).as_ref() {
            feed.set_input_mode(mode);
        }
    }

    /// A tecla de apertar para falar desceu ou subiu.
    pub fn talk(&self, talking: bool) {
        if let Some(feed) = lock(&self.microphone).as_ref() {
            feed.talk(talking);
        }
    }

    /// PCM `f32` estéreo intercalado a 48 kHz. Sem microfone aberto não faz nada.
    pub fn speak(&self, samples: &[f32]) {
        let feed = lock(&self.microphone).clone();

        if let Some(feed) = feed {
            feed.push(samples);
        }
    }

    /// Mutar também pausa o producer: é o que faz a sala desenhar o microfone fechado.
    pub async fn mute_microphone(&self, muted: bool) {
        let Some(feed) = lock(&self.microphone).clone() else {
            return;
        };

        feed.set_muted(muted);

        let producer_id = lock(&self.producers)
            .get(&Source::Mic)
            .and_then(|opened| opened.first().cloned());
        let acted = if muted {
            action::PAUSE_PRODUCER
        } else {
            action::RESUME_PRODUCER
        };

        if let Some(producer_id) = producer_id
            && let Err(failure) = self
                .session
                .client()
                .call(acted, json!({ "producerId": producer_id }))
                .await
        {
            tracing::warn!(%failure, muted, "a sala não soube do microfone");
        }

        self.announce_mine();
    }

    /// Abre a câmera no servidor. Quem captura é a interface, e o quadro entra por `show`.
    #[cfg(target_os = "macos")]
    pub async fn open_camera(&self, size: (u32, u32), frame_rate: u32) -> Result<()> {
        if lock(&self.camera).is_some() {
            return Ok(());
        }

        let producers = self.open(&[Some(Source::Camera)]).await?;
        let feed = tokio::task::block_in_place(|| {
            lock(&self.sending).video_feed(Source::Camera, size, frame_rate)
        });

        match feed {
            Ok(feed) => *lock(&self.camera) = Some(Arc::new(feed)),
            Err(failure) => {
                self.close(producers).await;

                return Err(failure);
            }
        }

        lock(&self.producers).insert(Source::Camera, producers);
        self.announce_mine();

        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub async fn close_camera(&self) {
        *lock(&self.camera) = None;

        self.retire(Source::Camera).await;
    }

    /// Um quadro da câmera, no buffer de GPU em que o sistema o entregou.
    #[cfg(target_os = "macos")]
    pub fn show(&self, surface: &capture::GpuSurface, timestamp_ns: u64) {
        let feed = lock(&self.camera).clone();

        if let Some(feed) = feed {
            feed.push(surface, timestamp_ns);
        }
    }

    pub fn deafen(&self, deafened: bool) {
        lock(&self.watching).deafen(deafened);
    }

    /// O som de uma transmissão, ligado ou desligado só para esta pessoa.
    pub fn mute_watched(&self, producer_id: &str, muted: bool) {
        lock(&self.watching).set_muted(producer_id, muted);
    }

    pub async fn leave(&self) {
        self.stop_sharing().await;
        self.close_microphone().await;

        #[cfg(target_os = "macos")]
        self.close_camera().await;

        lock(&self.watching).stop(None);

        if let Err(failure) = self.session.leave().await {
            tracing::warn!(%failure, "a sala não soube da saída");
        }
    }

    pub fn peers(&self) -> Value {
        json!({ "peers": self.session.peers() })
    }

    /// O que esta pessoa manda e o que ela tem permissão de mandar.
    pub fn mine(&self) -> Value {
        let microphone = lock(&self.microphone).clone();

        json!({
            "sharing": lock(&self.sending).screen.is_some(),
            "selfView": self.self_view.load(std::sync::atomic::Ordering::Relaxed),
            "mic": microphone.is_some(),
            "micMuted": microphone.is_some_and(|feed| feed.is_muted()),
            "camera": lock(&self.producers).contains_key(&Source::Camera),
            "canShare": self.session.can("stream"),
            "canSpeak": self.session.can("speak"),
            "canVideo": self.session.can("video"),
        })
    }

    /// Os cartões que a janela desenha — uma transmissão de vídeo por cartão — e o que está ao
    /// vivo sem estar sendo assistido (`pending`), para o botão "Assistir".
    pub fn tiles(&self) -> Value {
        let watching = lock(&self.watching);
        let paused = lock(&self.paused);
        let (mut tiles, mut pending) = (Vec::new(), Vec::new());

        for peer in &self.session.peers() {
            for producer in peer
                .producers
                .iter()
                .filter(|producer| producer.kind == "video")
            {
                let card = json!({
                    "producerId": producer.producer_id,
                    "peerId": peer.peer_id,
                    "label": peer.name,
                    "camera": producer.source == "camera",
                    "mine": peer.self_peer,
                    "paused": paused.contains(&producer.producer_id),
                    // O som que acompanha esta tela, para o botão de ouvir do cartão.
                    "audio": peer.producers.iter().find(|other| other.source == "screenAudio" && producer.source == "screen").map(|other| &other.producer_id),
                });

                if watching.is_watching(&producer.producer_id) {
                    tiles.push(card);
                } else if !peer.self_peer {
                    pending.push(card);
                }
            }
        }

        json!({ "tiles": tiles, "pending": pending })
    }

    /// Abre no servidor cada origem pedida e aponta o remetente para a porta que ele deu —
    /// fora do cadeado da captura e antes dela, porque resolver o endereço pode ir ao DNS.
    async fn open(&self, sources: &[Option<Source>]) -> Result<Vec<String>> {
        let mut producers = Vec::new();
        let mut last = Value::Null;

        for source in sources.iter().flatten() {
            let mut request = json!({
                "kind": if source.is_video() { "video" } else { "audio" },
                "source": source.name(),
            });

            merge(&mut request, lock(&self.sending).sfu_offer(*source));

            last = self
                .session
                .client()
                .call(action::PRODUCE_PLAIN, request)
                .await?;
            producers.push(text(&last, "producerId"));
        }

        let pointed = lock(&self.sending).use_sfu(
            &address_of(&last),
            decode(&last["srtpParameters"]["keyBase64"]),
        );

        if let Err(failure) = pointed {
            self.close(producers).await;

            return Err(failure);
        }

        Ok(producers)
    }

    async fn close(&self, producers: Vec<String>) {
        for producer_id in producers {
            if let Err(failure) = self
                .session
                .client()
                .call(action::CLOSE_PRODUCER, json!({ "producerId": producer_id }))
                .await
            {
                tracing::warn!(%failure, producer = %producer_id, "o producer não fechou no servidor");
            }
        }
    }

    /// Fecha no servidor o que uma origem abriu, e solta o remetente se foi a última.
    async fn retire(&self, source: Source) {
        let producers = lock(&self.producers).remove(&source).unwrap_or_default();

        self.close(producers).await;

        // A chave só é renovada com tudo parado: com o microfone de pé ela ainda está em uso.
        if lock(&self.producers).is_empty() {
            lock(&self.sending).release_if_idle();
        }

        self.announce_mine();
    }

    /// Depois de uma entrada nova (a carência do servidor expirou) ele não tem mais nada desta
    /// pessoa: o que estava aberto aqui é descartado, e o que ela transmitia sobe de novo.
    async fn republish(&self) {
        let screen = lock(&self.shared).take();
        let had_microphone = lock(&self.microphone).is_some();
        let broadcast = lock(&self.sending).screen.take();

        lock(&self.watching).stop(None);
        lock(&self.consumers).clear();
        lock(&self.paused).clear();

        if let Some(mut broadcast) = broadcast {
            let _ = tokio::task::block_in_place(|| broadcast.stop());
        }

        *lock(&self.microphone) = None;

        #[cfg(target_os = "macos")]
        {
            *lock(&self.camera) = None;
        }

        lock(&self.producers).clear();

        // A chave vai junto: o remetente novo recomeça a numeração, e a mesma chave com o
        // contador reiniciado repetiria o keystream.
        lock(&self.sending).renew_sfu_key();

        if let Some(config) = screen
            && let Err(failure) = self.share(config).await
        {
            tracing::warn!(%failure, "a tela não voltou depois da queda");
            self.tell("room.failed", json!({ "what": "share" }));
        }

        if had_microphone && let Err(failure) = self.reopen_microphone().await {
            tracing::warn!(%failure, "o microfone não voltou depois da queda");
            self.tell("room.failed", json!({ "what": "mic" }));
        }

        self.announce_mine();
    }

    /// Para tudo o que sobe e o que chega, sem falar com o servidor: o socket já se foi.
    fn stop_everything(&self) {
        let broadcast = lock(&self.sending).screen.take();

        if let Some(mut broadcast) = broadcast {
            let _ = tokio::task::block_in_place(|| broadcast.stop());
        }

        *lock(&self.shared) = None;
        *lock(&self.microphone) = None;

        #[cfg(target_os = "macos")]
        {
            *lock(&self.camera) = None;
        }

        lock(&self.producers).clear();
        lock(&self.consumers).clear();
        lock(&self.watching).stop(None);
    }

    fn announce_peers(&self) {
        self.tell("room.peers", self.peers());
    }

    fn announce_tiles(&self) {
        let tiles = self.tiles();

        if std::mem::replace(&mut *lock(&self.shown), tiles.clone()) != tiles {
            self.tell("room.tiles", tiles);
        }
    }

    fn announce_mine(&self) {
        self.tell("room.mine", self.mine());
    }

    fn tell(&self, event: &str, data: Value) {
        let _ = self
            .updates
            .send(json!({ "event": event, "channel": null, "data": data }).to_string());
    }
}

fn address_of(answer: &Value) -> String {
    format!(
        "{}:{}",
        answer["ip"].as_str().unwrap_or_default(),
        answer["port"]
    )
}

/// O `producePlain` recebe o pedido e a oferta no mesmo objeto (`{kind, source, ...offer}`).
fn merge(request: &mut Value, offer: Value) {
    let (Some(request), Some(offer)) = (request.as_object_mut(), offer.as_object()) else {
        return;
    };

    for (field, value) in offer {
        request.insert(field.clone(), value.clone());
    }
}

fn text(answer: &Value, field: &str) -> String {
    answer[field].as_str().unwrap_or_default().to_owned()
}

fn decode(value: &Value) -> Option<Vec<u8>> {
    STANDARD.decode(value.as_str()?).ok()
}

/// Um `Mutex` envenenado aqui é uma thread de captura que caiu. Seguir com o que se tem é
/// melhor do que derrubar a janela de quem está na sala.
fn lock<T>(cell: &Mutex<T>) -> MutexGuard<'_, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offer_and_the_request_go_up_in_the_same_object() {
        let mut request = json!({ "kind": "video", "source": "screen" });

        merge(
            &mut request,
            json!({ "rtpParameters": { "ssrc": 7 }, "srtpParameters": {} }),
        );

        assert_eq!(request["source"], "screen");
        assert_eq!(request["rtpParameters"]["ssrc"], 7);
        assert!(request.get("srtpParameters").is_some());
    }

    #[test]
    fn the_address_is_the_one_the_server_answered() {
        assert_eq!(
            address_of(&json!({ "ip": "203.0.113.7", "port": 40_123 })),
            "203.0.113.7:40123"
        );
    }
}
