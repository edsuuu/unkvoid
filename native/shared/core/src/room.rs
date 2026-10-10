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
//! `room.failed` (`watch`, `share`, `shareClosed`, `mic`, `serverMuted` e `camera` — a câmera
//! que o servidor derrubou, ou que não voltou depois de uma queda),
//! `room.watchers` quem assiste a cada tela,
//! `room.ping` a ida e volta até o servidor e
//! `room.level` o nível do microfone.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

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
use crate::sharing::{self, AudioFeed, Stall, StallWatch};
use crate::watching::{ArrivalWatch, Incoming, Media, Watching};

/// De quanto em quanto tempo o vigia confere a tela que esta pessoa transmite.
const GUARD_EVERY: Duration = Duration::from_secs(1);

/// De quanto em quanto tempo os números da transmissão vão para o log.
const NUMBERS_EVERY: Duration = Duration::from_secs(10);
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
    /// Os producers com o `consumePlain` no ar: marcados antes do `await`, para duas chamadas
    /// de `consume_all` ao mesmo tempo não abrirem dois consumers do mesmo producer.
    opening: Mutex<HashSet<String>>,
    /// O que a pessoa fechou de propósito: não reabre sozinho, só pelo "Assistir".
    closed: Mutex<HashSet<String>>,
    paused: Mutex<HashSet<String>>,
    /// Se o servidor está recebendo cada producer (`producerReceiving`): é o que separa a tela
    /// parada de quem transmite do caminho de chegada morto.
    receiving: Mutex<HashMap<String, bool>>,
    /// A janela está fora da vista (`set_away`), e o vídeo que ela pausou por isso — não pela
    /// pessoa: volta sozinho quando ela aparece.
    hidden: std::sync::atomic::AtomicBool,
    away: Mutex<HashSet<String>>,
    /// A transmissão em tela cheia, se há uma (`set_focus`).
    focus: Mutex<Option<String>>,
    /// Telas que a pessoa mandou assistir pelo "Assistir", acima do `screens_at_once`.
    chosen: Mutex<HashSet<String>>,
    /// O microfone mutado pela pessoa e o silenciado por um moderador (`serverMuted`): vale o
    /// que estiver ligado.
    user_muted: std::sync::atomic::AtomicBool,
    server_muted: std::sync::atomic::AtomicBool,
    /// "Ver o que a sala vê": assistir à própria tela, que custa um decodificador a mais.
    self_view: std::sync::atomic::AtomicBool,
    /// O `resend` está refazendo o que sobe: o `room.mine` sai uma vez só, no fim. No meio a
    /// tela já voltou e a câmera ainda não, e a interface do macOS fecharia a câmera que manda.
    resending: std::sync::atomic::AtomicBool,
    /// A receita da tela que está subindo, para republicar depois de uma queda longa.
    shared: Mutex<Option<CaptureConfig>>,
    /// A da câmera, pelo mesmo motivo: antes ela era a única que não voltava da queda.
    filming: Mutex<Option<CameraRecipe>>,
    /// Os cartões do último aviso: a interface só redesenha o palco quando eles mudam.
    shown: Mutex<Value>,
    /// O elenco do último aviso, para saber o que mudou e qual toque tocar.
    cast: Mutex<Vec<crate::models::Peer>>,
    updates: Sender<String>,
}

/// Como a câmera que está subindo foi aberta, para abri-la de novo depois de uma queda.
enum CameraRecipe {
    /// Capturada pelo núcleo (a webcam do Linux).
    Captured(CaptureConfig),
    /// Entregue pela interface quadro a quadro (o macOS): o tamanho e o ritmo do encoder.
    #[cfg(target_os = "macos")]
    Fed((u32, u32), u32),
}

/// Um producer marcado como "abrindo" enquanto o `consumePlain` dele está no ar. A marca sai
/// quando isto cai — deu certo, falhou ou a tarefa foi largada no meio —, senão o producer nunca
/// mais seria assistido.
struct Opening<'a> {
    marks: &'a Mutex<HashSet<String>>,
    producer_id: String,
}

impl<'a> Opening<'a> {
    fn mark(marks: &'a Mutex<HashSet<String>>, producer_id: &str) -> Option<Self> {
        lock(marks).insert(producer_id.to_owned()).then(|| Self { marks, producer_id: producer_id.to_owned() })
    }
}

impl Drop for Opening<'_> {
    fn drop(&mut self) {
        lock(self.marks).remove(&self.producer_id);
    }
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
            opening: Mutex::default(),
            closed: Mutex::default(),
            paused: Mutex::default(),
            receiving: Mutex::default(),
            hidden: std::sync::atomic::AtomicBool::new(false),
            away: Mutex::default(),
            focus: Mutex::default(),
            chosen: Mutex::default(),
            user_muted: std::sync::atomic::AtomicBool::new(false),
            server_muted: std::sync::atomic::AtomicBool::new(false),
            self_view: std::sync::atomic::AtomicBool::new(false),
            resending: std::sync::atomic::AtomicBool::new(false),
            shared: Mutex::default(),
            filming: Mutex::default(),
            shown: Mutex::default(),
            cast: Mutex::default(),
            updates,
        });

        tokio::spawn({
            let room = Arc::clone(&room);

            async move { room.run(events).await }
        });
        tokio::spawn(Self::guard_sending(Arc::downgrade(&room)));
        tokio::spawn(Self::guard_watching(Arc::downgrade(&room)));

        Ok((room, media))
    }

    /// O vigia do caminho de chegada (`ArrivalWatch`): de segundo em segundo confere se as telas
    /// que o servidor diz estar recebendo continuam chegando aqui, e refaz o caminho quando não.
    /// É o que o WebRTC faz provando a conexão a cada poucos segundos. Termina com a sala.
    async fn guard_watching(room: Weak<Self>) {
        let mut beat = tokio::time::interval(GUARD_EVERY);
        let mut arrival = ArrivalWatch::default();

        loop {
            beat.tick().await;

            let Some(room) = room.upgrade() else {
                return;
            };
            let consumed: Vec<String> = lock(&room.consumers).keys().cloned().collect();
            let paused = lock(&room.paused).clone();
            let away = lock(&room.away).clone();
            let receiving = lock(&room.receiving).clone();
            let screens: Vec<(String, u64, bool)> = consumed
                .into_iter()
                .filter(|producer_id| !paused.contains(producer_id) && !away.contains(producer_id))
                .filter_map(|producer_id| {
                    let packets = room.counters(&producer_id)?.received;
                    let flowing = receiving.get(&producer_id).copied().unwrap_or(false);

                    Some((producer_id, packets, flowing))
                })
                .collect();

            if arrival.tick(&screens, Instant::now()) {
                tracing::error!("assistir: o servidor recebe a tela e nada chega aqui há 5 s, refazendo o caminho de chegada");
                room.rewatch();
                room.settle().await;
            }
        }
    }

    /// O vigia do que esta pessoa transmite: de segundo em segundo confere se captura, encoder
    /// e envio continuam produzindo (`StallWatch`) e refaz captura e encoder no mesmo producer
    /// quando uma etapa para — quem assiste vê a imagem voltar no quadro-chave seguinte, sem a
    /// tela sumir da sala. Se é o servidor que parou de responder, o caminho até ele morreu, e
    /// tudo sobe de novo por outro (`resend`). Termina com a sala.
    async fn guard_sending(room: Weak<Self>) {
        let mut beat = tokio::time::interval(GUARD_EVERY);
        let mut watch: Option<StallWatch> = None;
        let mut numbers_at = Instant::now();

        loop {
            beat.tick().await;

            let Some(room) = room.upgrade() else {
                return;
            };
            let now = Instant::now();
            let stall = {
                let sending = lock(&room.sending);

                match sending.screen.as_ref().filter(|broadcast| !broadcast.is_muted()) {
                    Some(broadcast) => {
                        if now.duration_since(numbers_at) >= NUMBERS_EVERY {
                            broadcast.log_numbers();
                            numbers_at = now;
                        }

                        watch.get_or_insert_with(|| StallWatch::new(now)).tick(broadcast.counts(), now)
                    }
                    None => {
                        watch = None;

                        None
                    }
                }
            };

            match stall {
                Some(Stall::Transport) => tracing::error!("transmissão: o encoder devolve quadros e nada sai para a rede"),
                Some(stall) => {
                    let (source, on_hardware) = lock(&room.sending)
                        .screen
                        .as_ref()
                        .map_or((None, false), |broadcast| (Some(broadcast.source), broadcast.on_hardware()));

                    // A janela que fechou (o jogo que saiu) não volta refazendo: a transmissão
                    // para, e quem transmite fica sabendo — antes ela seguia "no ar" com o último
                    // quadro parado para quem assistia, e o vigia falhava a cada espera.
                    if source.is_some_and(|source| !capture::source_exists(source)) {
                        tracing::warn!("transmissão: a janela compartilhada foi fechada");
                        room.stop_sharing().await;
                        room.tell("room.failed", json!({ "what": "shareClosed" }));
                        watch = None;

                        continue;
                    }

                    // Minimizada ela não entrega quadro; refazer só custaria engasgos ao jogo.
                    if stall == Stall::Capture && source.is_some_and(capture::source_minimized) {
                        continue;
                    }

                    let fall_back = stall == Stall::Encoder && on_hardware && watch.as_ref().is_some_and(StallWatch::encoder_keeps_failing);

                    match stall {
                        _ if fall_back => tracing::error!("transmissão: o encoder da placa parou de novo, passando para o do processador"),
                        Stall::Encoder => tracing::error!("transmissão: o encoder parou de devolver quadros, refazendo captura e encoder"),
                        _ => tracing::warn!("transmissão: a captura parou de mandar quadros (tela parada ou captura travada), refazendo"),
                    }

                    let refreshed = if fall_back {
                        room.redo_screen(sharing::Broadcast::fall_back_to_cpu)
                    } else {
                        room.redo_screen(sharing::Broadcast::refresh)
                    };

                    match refreshed {
                        Ok(()) => {
                            if let Some(watch) = watch.as_mut() {
                                watch.restarted(Instant::now());
                            }
                        }
                        Err(error) => tracing::error!(error = %error, "transmissão: não deu para refazer a captura"),
                    }
                }
                None => {}
            }

            // A perda que não cede no piso da taxa desce um degrau de qualidade (720p, depois
            // 720p30); um minuto limpo no teto sobe um de volta.
            let (starved, roomy) = lock(&room.sending)
                .screen
                .as_ref()
                .map_or((false, false), |broadcast| (broadcast.starved(), broadcast.roomy()));

            if starved || roomy {
                let stepped = room.redo_screen(|broadcast| if starved { broadcast.step_down() } else { broadcast.step_up() });

                if let Err(error) = stepped {
                    tracing::error!(error = %error, "transmissão: o degrau de qualidade não abriu");
                }

                if let Some(watch) = watch.as_mut() {
                    watch.restarted(Instant::now());
                }
            }

            let lost = lock(&room.sending).lost_the_server();

            if lost {
                tracing::error!("transmissão: o servidor parou de responder, subindo de novo por outro caminho");
                room.resend().await;
                watch = None;
            }
        }
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
                    lock(&self.receiving).remove(producer_id);
                    lock(&self.away).remove(producer_id);
                    lock(&self.chosen).remove(producer_id);

                    // A tela cheia que acabou devolve as outras à vista.
                    let focused_left = lock(&self.focus).as_deref() == Some(producer_id);

                    if focused_left {
                        *lock(&self.focus) = None;
                        self.apply_view().await;
                    }
                }
                local::SESSION_LOST => self.tell("room.session", json!({ "state": "lost" })),
                local::SESSION_REJOINED => {
                    self.tell("room.session", json!({ "state": "rejoined" }));

                    if self.session.resumed() {
                        self.rewatch();
                    } else {
                        // Entrada nova (a carência do servidor expirou) perdeu tudo o que
                        // estava aberto lá: o que ficou aqui só atrapalha.
                        self.republish().await;
                    }

                    self.settle().await;
                }
                "producerDead" => self.died(&event.data).await,
                "serverMuted" => self.silenced(event.data["muted"].as_bool().unwrap_or(false)),
                "producerReceiving" => {
                    if let (Some(producer_id), Some(receiving)) =
                        (event.data["producerId"].as_str(), event.data["receiving"].as_bool())
                    {
                        lock(&self.receiving).insert(producer_id.to_owned(), receiving);
                    }
                }
                local::SESSION_GONE => self.tell("room.session", json!({ "state": "gone" })),
                // A sala acabou para esta sessão, e não volta: o que subia para, e a interface
                // tira a pessoa de lá com o motivo.
                "replaced" | "kicked" => {
                    self.stop_everything();
                    self.tell("room.session", json!({ "state": event.name }));
                }
                // Um moderador moveu esta pessoa: a sala acaba aqui e a interface entra no
                // destino (`to`, o canal) com um token novo, dizendo quem (`by`) a moveu.
                "moved" => {
                    self.stop_everything();
                    self.tell(
                        "room.session",
                        json!({ "state": "moved", "to": event.data["to"], "by": event.data["by"] }),
                    );
                }
                local::PING_MEASURED => self.tell(
                    "room.ping",
                    json!({ "ms": event.data, "bars": event.data.as_u64().map(signal_bars) }),
                ),
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
                    // ponytail: no macOS a própria câmera volta pelo SFU em vez de uma prévia
                    // local — custa um decoder e ~150 ms de atraso na própria imagem. A saída é
                    // um cartão com `AVCaptureVideoPreviewLayer`; o Windows e o Linux já têm a deles.
                    (mine && producer.source == "screen")
                        || (cfg!(target_os = "macos") && producer.source == "camera")
                } else {
                    !closed.contains(&producer.producer_id)
                }
            })
            .map(|(_, producer)| producer.clone())
            .collect();

        let chosen = lock(&self.chosen).clone();
        let watched = |producer: &ProducerInfo| {
            lock(&self.watching).is_watching(&producer.producer_id) || lock(&self.opening).contains(&producer.producer_id)
        };
        let mut screens = wanted.iter().filter(|producer| producer.source == "screen" && watched(producer)).count();

        for producer in &wanted {
            // Além do limite, a tela fica no "Assistir": a pessoa escolhe qual abrir.
            if producer.source == "screen" && !watched(producer) && !chosen.contains(&producer.producer_id) {
                if screens >= screens_at_once() {
                    continue;
                }

                screens += 1;
            }

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

        // Outra tarefa já está abrindo este: o segundo consumer ficaria órfão, com a banda dele
        // correndo para sempre e pausar e fechar agindo no outro. E a sala que já saiu não abre
        // nada: o `settle` da entrada corre ao lado de um `leave` logo em seguida.
        let Some(_opening) = Opening::mark(&self.opening, &producer.producer_id).filter(|_| !self.session.has_left()) else {
            return Ok(());
        };

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

        // Fechada pela pessoa, ou a sala deixada, enquanto o pedido estava no ar.
        if lock(&self.closed).contains(&producer.producer_id) || self.session.has_left() {
            let _ = self
                .session
                .client()
                .call(action::CLOSE_CONSUMER, json!({ "consumerId": consumer_id }))
                .await;

            return Ok(());
        }

        lock(&self.receiving).insert(producer.producer_id.clone(), answer["receiving"].as_bool().unwrap_or(false));
        let server_key = decode(&answer["srtpParameters"]["keyBase64"])
            .ok_or_else(|| anyhow!("consumidor sem chave"))?;
        let kind = answer["kind"].as_str().unwrap_or(&producer.kind).to_owned();
        let source = answer["source"]
            .as_str()
            .unwrap_or(&producer.source)
            .to_owned();

        let follows = (source == "screenAudio").then(|| self.screen_beside(&producer.producer_id)).flatten();
        let started = lock(&self.watching).start(Incoming {
            producer_id: producer.producer_id.clone(),
            kind: &kind,
            address: &address,
            server_key: &server_key,
            payload_type: answer["payloadType"].as_u64().unwrap_or_default() as u8,
            ssrc: answer["ssrc"].as_u64().map(|ssrc| ssrc as u32),
            always_muted: source == "screenAudio",
            rtx: crate::watching::rtx_of(&answer),
            follows,
        });

        if let Err(failure) = started {
            let _ = self
                .session
                .client()
                .call(action::CLOSE_CONSUMER, json!({ "consumerId": consumer_id }))
                .await;

            return Err(failure);
        }

        // Pausado antes de o caminho ser refeito continua pausado: o consumer nasce assim. E o
        // vídeo que chega com a janela fora da vista espera ela voltar.
        let paused = lock(&self.paused).contains(&producer.producer_id);
        let away = kind == "video" && self.unseen(&producer.producer_id);

        if away {
            lock(&self.away).insert(producer.producer_id.clone());
        }

        if !paused && !away {
            self.session
                .client()
                .call(
                    action::RESUME_CONSUMER,
                    json!({ "consumerId": consumer_id }),
                )
                .await?;
        }

        lock(&self.consumers).insert(producer.producer_id.clone(), consumer_id);

        Ok(())
    }

    /// A tela da pessoa que manda este producer: é ela que o som da tela acompanha.
    fn screen_beside(&self, producer_id: &str) -> Option<String> {
        let peer = self.session.peers().into_iter().find(|peer| peer.producers.iter().any(|other| other.producer_id == producer_id))?;

        peer.producers.into_iter().find(|other| other.source == "screen").map(|screen| screen.producer_id)
    }

    /// Para de receber uma transmissão sem sair da sala; ela continua ao vivo para os outros.
    pub async fn close_watched(&self, producer_id: &str) {
        lock(&self.watching).stop(Some(producer_id));
        lock(&self.closed).insert(producer_id.to_owned());
        lock(&self.paused).remove(producer_id);

        // Fechar a tela que estava em tela cheia devolve as outras à vista.
        let focused = lock(&self.focus).as_deref() == Some(producer_id);

        if focused {
            *lock(&self.focus) = None;
            self.apply_view().await;
        }

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
                lock(&self.chosen).insert(producer_id.to_owned());
            }
            None => {
                lock(&self.closed).clear();
                lock(&self.chosen).extend(
                    self.session
                        .peers()
                        .iter()
                        .flat_map(|peer| peer.producers.iter())
                        .map(|producer| producer.producer_id.clone()),
                );
            }
        }

        self.consume_all().await;
        self.announce_tiles();
    }

    /// A janela saiu da vista (minimizada, escondida) ou voltou. Fora da vista, o vídeo do que
    /// se assiste é pausado no servidor: imagem que ninguém vê não gasta banda nem a CPU que o
    /// jogo quer — o app em React fazia o mesmo. Na volta ele é retomado, e o `resumeConsumer`
    /// pede o quadro-chave. O som continua. Chamar de novo com o mesmo valor não faz nada.
    pub async fn set_away(&self, away: bool) {
        if self.hidden.swap(away, std::sync::atomic::Ordering::Relaxed) == away {
            return;
        }

        self.apply_view().await;
    }

    /// Uma tela em tela cheia (`Some`) ou nenhuma: com uma em tela cheia, as outras não estão à
    /// vista, e o vídeo delas pausa no servidor como no app em React — decodificar três telas
    /// atrás de uma só gasta a CPU do jogo à toa.
    pub async fn set_focus(&self, producer_id: Option<String>) {
        if std::mem::replace(&mut *lock(&self.focus), producer_id.clone()) == producer_id {
            return;
        }

        self.apply_view().await;
    }

    /// Se uma transmissão de vídeo não está à vista: a janela está fora, ou outra está em tela
    /// cheia.
    fn unseen(&self, producer_id: &str) -> bool {
        self.hidden.load(std::sync::atomic::Ordering::Relaxed) || lock(&self.focus).as_deref().is_some_and(|focused| focused != producer_id)
    }

    /// Pausa no servidor o vídeo que deixou de estar à vista e retoma o que voltou — sem mexer no
    /// que a pessoa pausou.
    async fn apply_view(&self) {
        let paused = lock(&self.paused).clone();
        let unseen: HashSet<String> = self
            .session
            .peers()
            .iter()
            .flat_map(|peer| peer.producers.iter())
            .filter(|producer| producer.kind == "video" && !paused.contains(&producer.producer_id) && self.unseen(&producer.producer_id))
            .map(|producer| producer.producer_id.clone())
            .collect();
        let current = lock(&self.away).clone();
        let consumers = lock(&self.consumers).clone();
        let changes = unseen
            .difference(&current)
            .map(|producer_id| (producer_id.clone(), true))
            .chain(current.difference(&unseen).map(|producer_id| (producer_id.clone(), false)));

        for (producer_id, away) in changes.collect::<Vec<_>>() {
            let acted = if away { action::PAUSE_CONSUMER } else { action::RESUME_CONSUMER };
            let done = match consumers.get(&producer_id) {
                Some(consumer_id) => self.session.client().call(acted, json!({ "consumerId": consumer_id })).await.is_ok(),
                None => true,
            };

            if done {
                let mut away_set = lock(&self.away);

                if away {
                    away_set.insert(producer_id);
                } else {
                    away_set.remove(&producer_id);
                }
            }
        }
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

    /// Troca resolução, quadros por segundo **e a tela** com a transmissão no ar, sem
    /// fechar o producer. Trocar de monitor parando e começando de novo esbarraria no SSRC
    /// repetido da sala, e a transmissão morreria no lugar de mudar de tela.
    pub async fn change_quality(
        &self,
        quality: capture::Quality,
        frame_rate: u32,
        source: Option<capture::CaptureSource>,
    ) -> Result<()> {
        let changed = self.redo_screen(|broadcast| broadcast.restart(quality, frame_rate, source));

        if let Some(config) = lock(&self.shared).as_mut() {
            config.quality = quality;
            config.frame_rate = frame_rate;

            if let Some(source) = source {
                config.source = source;
            }
        }

        changed
    }

    /// Refaz a tela no ar (`work`) fora do cadeado de quem transmite: reabrir captura e encoder
    /// leva de 0,3 a 2 s em PC fraco, e com o cadeado na mão a sala inteira esperava atrás — a
    /// lista, os cartões, o próprio ping. Se a pessoa parou de transmitir no meio, a transmissão
    /// refeita para aqui em vez de voltar ao ar.
    ///
    /// ponytail: um `change_quality` que chegue durante um refazer do vigia acha a tela fora e
    /// só troca a receita; a troca vale no próximo refazer. Um cadeado assíncrono em volta de
    /// tudo o que abre e fecha origem resolveria os dois.
    fn redo_screen(&self, work: impl FnOnce(&mut sharing::Broadcast) -> Result<()>) -> Result<()> {
        let Some(mut broadcast) = lock(&self.sending).screen.take() else {
            return Ok(());
        };
        let done = tokio::task::block_in_place(|| work(&mut broadcast));
        let still_sharing = lock(&self.shared).is_some();
        let leftover = {
            let mut sending = lock(&self.sending);

            if still_sharing && sending.screen.is_none() {
                sending.screen = Some(broadcast);

                None
            } else {
                Some(broadcast)
            }
        };

        if let Some(mut leftover) = leftover {
            let _ = tokio::task::block_in_place(|| leftover.stop());
        }

        done
    }

    /// A receita da tela no ar. É ela que diz se uma troca no seletor cabe no
    /// `change_quality` (mesmo áudio) ou se a transmissão tem de parar e recomeçar.
    pub fn sharing_recipe(&self) -> Option<CaptureConfig> {
        lock(&self.shared).clone()
    }

    /// Compartilhar a tela. Abre a origem no servidor e só então captura: sem o
    /// `producePlain` não há porta para onde mandar, e o quadro sairia no vazio.
    pub async fn share(&self, config: CaptureConfig) -> Result<()> {
        if lock(&self.shared).is_some() || lock(&self.sending).screen.is_some() {
            return Ok(());
        }

        let audio = config.capture_audio.then_some(Source::ScreenAudio);
        let producers = self.open(&[Some(Source::Screen), audio]).await?;
        let recipe = config.clone();

        // Fora do cadeado: abrir captura e encoder leva até segundos, e a sala não espera.
        let launch = lock(&self.sending).launcher();

        match tokio::task::block_in_place(|| launch(config, Some(Source::Screen), audio)) {
            Ok(broadcast) => lock(&self.sending).screen = Some(broadcast),
            Err(failure) => {
                self.close(producers).await;

                return Err(failure);
            }
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

    /// A câmera que a própria captura abre e codifica: no Linux o GStreamer lê a webcam e já
    /// entrega H.264 (`capture::captures_cameras`). No macOS a câmera vem pronta da interface,
    /// por `open_camera`.
    pub async fn open_captured_camera(&self, config: CaptureConfig) -> Result<()> {
        if !capture::captures_cameras() {
            return Err(anyhow!("a captura deste sistema não abre câmera"));
        }

        if lock(&self.sending).camera.is_some() {
            return Ok(());
        }

        let producers = self.open(&[Some(Source::Camera)]).await?;
        let recipe = config.clone();
        let started = tokio::task::block_in_place(|| {
            let mut sending = lock(&self.sending);
            let broadcast = sending.start(config, Some(Source::Camera), None)?;

            sending.camera = Some(broadcast);

            anyhow::Ok(())
        });

        if let Err(failure) = started {
            self.close(producers).await;

            return Err(failure);
        }

        lock(&self.producers).insert(Source::Camera, producers);
        *lock(&self.filming) = Some(CameraRecipe::Captured(recipe));
        self.announce_mine();

        Ok(())
    }

    pub async fn close_captured_camera(&self) {
        *lock(&self.filming) = None;

        let broadcast = lock(&self.sending).camera.take();

        if let Some(mut broadcast) = broadcast {
            let _ = tokio::task::block_in_place(|| broadcast.stop());
        }

        self.retire(Source::Camera).await;
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
        // O microfone que reabre (depois de uma queda) não fura o silêncio de um moderador.
        feed.set_muted(self.server_muted.load(std::sync::atomic::Ordering::Relaxed));
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

        self.user_muted.store(muted, std::sync::atomic::Ordering::Relaxed);

        // Silenciado por um moderador fica silenciado: o servidor recusa retomar o producer.
        if self.server_muted.load(std::sync::atomic::Ordering::Relaxed) {
            feed.set_muted(true);

            if !muted {
                self.tell("room.failed", json!({ "what": "serverMuted" }));
            }

            self.announce_mine();

            return;
        }

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

        *lock(&self.filming) = Some(CameraRecipe::Fed(size, frame_rate));
        lock(&self.producers).insert(Source::Camera, producers);
        self.consume_all().await;
        self.announce_tiles();
        self.announce_mine();

        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub async fn close_camera(&self) {
        *lock(&self.camera) = None;
        *lock(&self.filming) = None;

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

    /// Se o som de uma transmissão está calado aqui agora. `None` enquanto ele não é assistido.
    pub fn is_watched_muted(&self, producer_id: &str) -> Option<bool> {
        lock(&self.watching).is_muted(producer_id)
    }

    pub async fn leave(&self) {
        self.stop_sharing().await;
        self.close_microphone().await;

        #[cfg(target_os = "macos")]
        self.close_camera().await;

        // A webcam do Linux é captura do núcleo: sem isto ela seguia filmando (e com a luz
        // acesa) depois da saída, até a interface largar a sala.
        self.close_captured_camera().await;

        lock(&self.watching).stop(None);

        if let Err(failure) = self.session.leave().await {
            tracing::warn!(%failure, "a sala não soube da saída");
        }
    }

    /// O que aconteceu com o vídeo de uma transmissão assistida: recebidos, recuperados
    /// e perdidos. É a linha de números do cartão.
    pub fn counters(&self, producer_id: &str) -> Option<media::Counters> {
        lock(&self.watching).counters(producer_id)
    }

    /// A tela de uma transmissão quebrou do lado de quem assiste (quadro largado,
    /// decodificador que falhou): pede o keyframe na hora, como o WebRTC do navegador faz,
    /// em vez de a imagem ficar parada até o keyframe periódico.
    pub fn request_keyframe(&self, producer_id: &str) {
        lock(&self.watching).request_keyframe(producer_id);
    }

    /// Quando a sala começou, desde a primeira pessoa: o relógio da barra conta daí. `None`
    /// num SFU que ainda não diz.
    pub fn started(&self) -> Option<std::time::Instant> {
        self.session.started()
    }

    pub fn peers(&self) -> Value {
        json!({ "peers": self.session.peers() })
    }

    /// O que esta pessoa manda e o que ela tem permissão de mandar.
    pub fn mine(&self) -> Value {
        let microphone = lock(&self.microphone).clone();

        json!({
            // Pela receita, e não pela transmissão: enquanto o vigia a refaz ela está fora do
            // cadeado, e a interface piscaria "parou de transmitir".
            "sharing": lock(&self.shared).is_some(),
            "selfView": self.self_view.load(std::sync::atomic::Ordering::Relaxed),
            "mic": microphone.is_some(),
            "micMuted": microphone.is_some_and(|feed| feed.is_muted()),
            "serverMuted": self.server_muted.load(std::sync::atomic::Ordering::Relaxed),
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

            // A tela abriu e o som dela não: sem fechar a tela aqui, a sala via um cartão vazio
            // até o servidor desistir dele em 30 s.
            last = match self.session.client().call(action::PRODUCE_PLAIN, request).await {
                Ok(answer) => answer,
                Err(failure) => {
                    self.close(producers).await;

                    return Err(failure);
                }
            };
            producers.push(text(&last, "producerId"));

            self.note_own_producer("newProducer", &text(&last, "producerId"), source);
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

    /// O SFU avisa a sala inteira de um producer novo, menos quem o abriu. Sem o próprio
    /// producer no elenco, "ver o que a sala vê" não teria o que assistir.
    fn note_own_producer(&self, event: &str, producer_id: &str, source: &Source) {
        let Some(own) = self.session.peers().into_iter().find(|peer| peer.self_peer) else {
            return;
        };

        self.session.apply(&Event {
            name: event.to_owned(),
            channel: None,
            data: json!({
                "peerId": own.peer_id,
                "producerId": producer_id,
                "kind": if source.is_video() { "video" } else { "audio" },
                "source": source.name(),
            }),
        });
    }

    async fn close(&self, producers: Vec<String>) {
        for producer_id in producers {
            // Quem se assistia deixa de se assistir junto com a transmissão.
            lock(&self.watching).stop(Some(&producer_id));
            lock(&self.consumers).remove(&producer_id);

            self.session.apply(&Event {
                name: "producerClosed".to_owned(),
                channel: None,
                data: json!({ "peerId": self.session.peers().into_iter().find(|peer| peer.self_peer).map(|peer| peer.peer_id), "producerId": producer_id }),
            });

            if let Err(failure) = self
                .session
                .client()
                .call(action::CLOSE_PRODUCER, json!({ "producerId": producer_id }))
                .await
            {
                tracing::warn!(%failure, producer = %producer_id, "o producer não fechou no servidor");
            }
        }

        self.announce_tiles();
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
        lock(&self.watching).stop(None);
        lock(&self.consumers).clear();
        lock(&self.paused).clear();
        lock(&self.away).clear();
        lock(&self.producers).clear();

        self.resend().await;
    }

    /// Sobe de novo, por um remetente novo, a tela e o microfone que estavam no ar; o que ainda
    /// estiver aberto no servidor fecha antes. O microfone mutado volta mutado.
    ///
    /// ponytail: parar a tela durante os segundos em que isto roda pode trazê-la de volta. A
    /// saída é um cadeado assíncrono em volta de tudo o que abre e fecha origem.
    async fn resend(&self) {
        let screen = lock(&self.shared).take();
        let microphone = lock(&self.microphone).take();
        let camera = lock(&self.filming).take();
        let broadcasts = {
            let mut sending = lock(&self.sending);

            [sending.screen.take(), sending.camera.take()]
        };

        self.resending.store(true, std::sync::atomic::Ordering::Relaxed);

        for mut broadcast in broadcasts.into_iter().flatten() {
            let _ = tokio::task::block_in_place(|| broadcast.stop());
        }

        #[cfg(target_os = "macos")]
        {
            *lock(&self.camera) = None;
        }

        let producers: Vec<String> = lock(&self.producers).drain().flat_map(|(_, opened)| opened).collect();

        self.close(producers).await;

        // A chave vai junto: o remetente novo recomeça a numeração, e a mesma chave com o
        // contador reiniciado repetiria o keystream. É também a chave nova que faz o servidor
        // trocar o transporte.
        lock(&self.sending).renew_sfu_key();

        if let Some(config) = screen
            && let Err(failure) = self.share(config).await
        {
            tracing::warn!(%failure, "a tela não voltou depois da queda");
            self.tell("room.failed", json!({ "what": "share" }));
        }

        if let Some(previous) = microphone {
            match self.reopen_microphone().await {
                Ok(()) if previous.is_muted() => self.mute_microphone(true).await,
                Ok(()) => {}
                Err(failure) => {
                    tracing::warn!(%failure, "o microfone não voltou depois da queda");
                    self.tell("room.failed", json!({ "what": "mic" }));
                }
            }
        }

        // A câmera volta como a tela. No macOS a interface segue entregando quadros por `show`,
        // e eles passam a subir pelo remetente novo assim que ele abre.
        let reopened = match camera {
            Some(CameraRecipe::Captured(config)) => Some(self.open_captured_camera(config).await),
            #[cfg(target_os = "macos")]
            Some(CameraRecipe::Fed(size, frame_rate)) => Some(self.open_camera(size, frame_rate).await),
            None => None,
        };

        if let Some(Err(failure)) = reopened {
            tracing::warn!(%failure, "a câmera não voltou depois da queda");
            self.tell("room.failed", json!({ "what": "camera" }));
        }

        self.resending.store(false, std::sync::atomic::Ordering::Relaxed);
        self.announce_mine();
    }

    /// A conexão caiu e voltou na mesma sessão. Se foi o endereço da pessoa que mudou (o
    /// provedor reconectou, o roteador reiniciou), o servidor continua mandando para o antigo e
    /// descarta o que vem do novo: a imagem pararia ali até ela sair da sala. A chave nova faz
    /// o servidor abrir outro transporte de chegada, e o `settle` seguinte assiste tudo de
    /// novo. Custa um quadro-chave por tela, e só a quem voltou.
    fn rewatch(&self) {
        lock(&self.watching).renew();
        lock(&self.consumers).clear();
    }

    /// Um moderador silenciou (ou devolveu) o microfone desta pessoa. O servidor já pausou o
    /// producer; aqui o microfone cala de verdade e a interface mostra — antes ele seguia aberto
    /// na tela, falando para ninguém, e desmutar falhava calado. Como no app em React.
    fn silenced(&self, muted: bool) {
        self.server_muted.store(muted, std::sync::atomic::Ordering::Relaxed);

        if let Some(feed) = lock(&self.microphone).as_ref() {
            feed.set_muted(muted || self.user_muted.load(std::sync::atomic::Ordering::Relaxed));
        }

        if muted {
            self.tell("room.failed", json!({ "what": "serverMuted" }));
        }

        self.announce_mine();
    }

    /// O servidor fechou a tela ou o microfone desta pessoa: nada chegou lá em 30 s, ou a
    /// permissão caiu. Seguir capturando seria transmitir para o vazio achando que está no ar.
    async fn died(&self, data: &Value) {
        let producer_id = data["producerId"].as_str().unwrap_or_default();

        // O aviso de um producer que já foi trocado não derruba o que subiu no lugar dele.
        if !lock(&self.producers).values().flatten().any(|opened| opened == producer_id) {
            return;
        }

        match data["source"].as_str().and_then(Source::parse) {
            Some(Source::Screen) => {
                tracing::warn!(reason = ?data["reason"].as_str(), "o servidor fechou a tela");
                self.stop_sharing().await;
                self.tell("room.failed", json!({ "what": "share" }));
            }
            Some(Source::Mic) => {
                tracing::warn!(reason = ?data["reason"].as_str(), "o servidor fechou o microfone");
                self.close_microphone().await;
                self.tell("room.failed", json!({ "what": "mic" }));
            }
            #[cfg(target_os = "linux")]
            Some(Source::Camera) => {
                tracing::warn!(reason = ?data["reason"].as_str(), "o servidor fechou a câmera");
                self.close_captured_camera().await;
                self.tell("room.failed", json!({ "what": "camera" }));
            }
            _ => {}
        }
    }

    /// Para tudo o que sobe e o que chega, sem falar com o servidor: o socket já se foi. A
    /// câmera do Linux também: expulsa, movida ou substituída, a pessoa não filma para ninguém.
    fn stop_everything(&self) {
        let broadcasts = {
            let mut sending = lock(&self.sending);

            [sending.screen.take(), sending.camera.take()]
        };

        for mut broadcast in broadcasts.into_iter().flatten() {
            let _ = tokio::task::block_in_place(|| broadcast.stop());
        }

        *lock(&self.shared) = None;
        *lock(&self.microphone) = None;
        *lock(&self.filming) = None;

        #[cfg(target_os = "macos")]
        {
            *lock(&self.camera) = None;
        }

        lock(&self.producers).clear();
        lock(&self.consumers).clear();
        lock(&self.watching).stop(None);
    }

    /// O elenco, o toque da troca — `room.chime` com `joined`, `left`, `streamStarted` ou
    /// `streamStopped` (ver `chimes.rs`) — e o aviso dela, `room.notice`. A interface só toca
    /// e mostra.
    fn announce_peers(&self) {
        let peers = self.session.peers();
        let before = std::mem::replace(&mut *lock(&self.cast), peers.clone());

        self.tell("room.peers", json!({ "peers": peers }));

        if let Some(chime) = crate::chimes::Chime::after(&before, &peers) {
            self.tell("room.chime", json!({ "chime": chime }));
        }

        if let Some(text) = crate::chimes::Chime::notice_after(&before, &peers) {
            self.tell("room.notice", json!({ "text": text }));
        }
    }

    fn announce_tiles(&self) {
        let tiles = self.tiles();

        if std::mem::replace(&mut *lock(&self.shown), tiles.clone()) != tiles {
            self.tell("room.tiles", tiles);
        }
    }

    fn announce_mine(&self) {
        if self.resending.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }

        self.tell("room.mine", self.mine());
    }

    fn tell(&self, event: &str, data: Value) {
        let _ = self
            .updates
            .send(json!({ "event": event, "channel": null, "data": data }).to_string());
    }
}

/// Quantas telas abrem sozinhas: duas em PC de até quatro núcleos, quatro nos outros, como o app
/// em React. Cada tela 1080p60 decodificada custa perto de um núcleo; as que passam disso ficam
/// no "Assistir".
fn screens_at_once() -> usize {
    if std::thread::available_parallelism().map_or(4, std::num::NonZero::get) <= 4 { 2 } else { 4 }
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

/// Quantas barrinhas de sinal a ida e volta até o SFU merece: 4 é verde, 3 amarelo, 2 laranja
/// e 1 vermelho. Os cortes são os de uma chamada de voz — até 80 ms ninguém percebe, de 150
/// em diante a conversa começa a atropelar, e acima de 250 já se fala por cima do outro.
/// O que esta pessoa manda e pode mandar, como o `room.mine` anuncia.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mine {
    pub sharing: bool,
    pub self_view: bool,
    pub mic: bool,
    pub mic_muted: bool,
    pub camera: bool,
    pub can_share: bool,
    pub can_speak: bool,
    pub can_video: bool,
}

impl Mine {
    /// O microfone desenhado como desligado, dentro da sala: mudo por escolha, sem
    /// permissão de falar, ou fechado de verdade — e não só ainda abrindo. É a conta do
    /// macOS: entre o clique no canal e o microfone abrir o botão não tem o que mostrar, e
    /// pintá-lo de mudo nesse meio segundo era o pisca que ninguém pediu. Fora da sala quem
    /// manda é o mudo guardado, e quem chama escolhe.
    pub fn mic_shown_off(self, opening: bool) -> bool {
        self.mic_muted || !self.can_speak || (!self.mic && !opening)
    }
}

pub fn signal_bars(round_trip_ms: u64) -> u8 {
    match round_trip_ms {
        0..=80 => 4,
        81..=150 => 3,
        151..=250 => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_mic_is_drawn_on_while_it_is_still_opening() {
        let joined = Mine { can_speak: true, ..Mine::default() };

        assert!(!joined.mic_shown_off(true));
        assert!(joined.mic_shown_off(false));
    }

    #[test]
    fn a_muted_or_voiceless_mic_is_drawn_off_even_while_opening() {
        let muted = Mine { can_speak: true, mic: true, mic_muted: true, ..Mine::default() };
        let voiceless = Mine { can_speak: false, mic: false, ..Mine::default() };

        assert!(muted.mic_shown_off(true));
        assert!(voiceless.mic_shown_off(true));
    }

    #[test]
    fn an_open_mic_that_can_speak_is_drawn_on() {
        let open = Mine { can_speak: true, mic: true, ..Mine::default() };

        assert!(!open.mic_shown_off(false));
    }

    #[test]
    fn the_announced_mine_is_read_with_its_camel_case_names() {
        let mine: Mine = serde_json::from_str(
            r#"{"sharing":true,"selfView":false,"mic":true,"micMuted":true,"camera":false,"canShare":true,"canSpeak":true,"canVideo":false}"#,
        )
        .unwrap();

        assert!(mine.sharing && mine.mic && mine.mic_muted && mine.can_share && mine.can_speak);
        assert!(!mine.camera && !mine.can_video && !mine.self_view);
    }

    #[test]
    fn the_signal_loses_a_bar_at_each_cut() {
        assert_eq!([12, 80, 81, 150, 151, 250, 251, 900].map(super::signal_bars), [4, 4, 3, 3, 2, 2, 1, 1]);
    }

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
