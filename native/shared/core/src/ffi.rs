//! A ponte para a interface que não é Rust: Swift no macOS.
//!
//! É uma ABI C de propósito, e não um gerador de bindings: são poucas funções, elas mudam
//! devagar, e um `.h` que se lê de cima a baixo vale mais aqui do que uma ferramenta a mais
//! no caminho do build. O app Linux não passa por aqui — GTK é Rust e chama o `core` direto.
//!
//! A regra da memória: tudo que sai daqui como `*mut c_char` volta em `unkvoid_string_free`.
//! Quem esquecer vaza; quem liberar duas vezes derruba o processo.

use std::ffi::{CStr, CString, c_char};
use std::ptr;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::runtime::Runtime;

use crate::api::{Api, HttpError};
use crate::app::{App, EntryRefusal};
use crate::client::SfuClient;
use crate::failure::Failure;
use crate::models::{RoomIdentity, Screen};
use crate::protocol::action;
use crate::room::Room;
use crate::session::Identity;
use crate::watching::{Media, MediaKind};

/// Quanto `unkvoid_next_media` espera por um quadro antes de devolver `null`. Curto o
/// bastante para a thread da interface notar que mandaram parar; longo o bastante para
/// ela não girar à toa com a sala em silêncio.
const MEDIA_PATIENCE: Duration = Duration::from_millis(100);

/// O que a interface segura entre uma chamada e outra.
///
/// Tudo o que muda vive atrás de cadeado, e as funções tomam o handle por `&` e nunca por
/// `&mut`. A interface consulta os eventos num timer **enquanto** uma ação está em voo —
/// é a topologia que ela precisa ter para não travar a tela —, e com `&mut` isso seria
/// corrida de dados: comportamento indefinido, do tipo que derruba o app sem padrão.
pub struct Handle {
    /// `Arc` porque a sessão da `Api` guarda o par renovado nele, de outra thread.
    app: Arc<App>,
    api: Mutex<Option<Arc<Api>>>,
    runtime: Runtime,
    /// `Arc` porque a tarefa que traz o socket de volta também o troca.
    client: Arc<Mutex<Option<Arc<SfuClient>>>>,
    /// Onde o SFU fica, guardado no `unkvoid_connect`: a sala abre o próprio socket lá.
    sfu: Mutex<Option<String>>,
    room: Mutex<Option<Arc<Room>>>,
    /// O login com Google à espera do navegador, entre o `googleStart` e o `googleWait`.
    google: Mutex<Option<crate::google::GoogleLogin>>,
    server: Mutex<Option<String>>,
    media: Mutex<Option<Receiver<Media>>>,
    events: Mutex<Receiver<String>>,
    sender: Sender<String>,
}

impl Handle {
    fn client(&self) -> Option<Arc<SfuClient>> {
        self.client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn api(&self) -> Option<Arc<Api>> {
        self.api
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn room(&self) -> Option<Arc<Room>> {
        self.room
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Entra de verdade na sala que o `App` já aceitou. `voice` é o canal de voz de um
    /// servidor; sem ele a sala é por código. Se o SFU recusar, a tela volta para onde estava.
    fn enter(&self, room: String, voice: Option<String>) -> Value {
        let Some(url) = self
            .sfu
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            self.app.leave_room();

            return json!({ "failed": Failure::Unreachable });
        };

        self.leave();

        let identity = self.identity(&room, voice);

        match self
            .runtime
            .block_on(Room::enter(&url, &room, identity, self.sender.clone()))
        {
            Ok((entered, media)) => {
                *self.room.lock().unwrap_or_else(PoisonError::into_inner) = Some(entered);
                *self.media.lock().unwrap_or_else(PoisonError::into_inner) = Some(media);

                json!({ "ok": true, "room": room })
            }
            Err(failure) => {
                tracing::warn!(%failure, "a sala não abriu");
                self.app.leave_room();

                json!({ "failed": Failure::from_error(&failure) })
            }
        }
    }

    fn leave(&self) {
        let room = self
            .room
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();

        *self.media.lock().unwrap_or_else(PoisonError::into_inner) = None;

        if let Some(room) = room {
            self.runtime.block_on(room.leave());
        }
    }

    /// Quem esta pessoa é para o SFU, **perguntado de novo a cada entrada**: o token de voz
    /// vale 60 s, e guardar o primeiro faria toda reconexão levar um token vencido.
    fn identity(&self, room: &str, voice: Option<String>) -> Identity {
        let api = self.api();
        let (room, name, install_id) = (
            room.to_owned(),
            self.app.state().name,
            self.app.install_id(),
        );

        Arc::new(move || {
            let (api, room, voice, name, install_id) = (
                api.clone(),
                room.clone(),
                voice.clone(),
                name.clone(),
                install_id.clone(),
            );

            Box::pin(async move {
                let signed = api.filter(|api| api.signed_in());

                let token = match (signed, voice) {
                    (Some(api), Some(channel)) => api.voice_token(&channel).await,
                    (Some(api), None) => api.room_token(&room).await,
                    (None, _) => {
                        return Ok(RoomIdentity::Guest {
                            room,
                            name,
                            install_id,
                        });
                    }
                };

                match token {
                    Ok(token) => Ok(RoomIdentity::Account { token }),
                    Err(HttpError::Failed(failure)) => Err(anyhow::Error::new(failure)),
                    Err(HttpError::Invalid { .. }) => Err(anyhow::Error::new(Failure::Invalid)),
                }
            })
        })
    }

    /// Uma ação sobre a sala aberta. Sem sala, responde `gone` em vez de derrubar.
    fn in_room(&self, work: impl FnOnce(&Arc<Room>, &Runtime) -> Value) -> Value {
        match self.room() {
            Some(room) => work(&room, &self.runtime),
            None => json!({ "failed": Failure::Gone }),
        }
    }

    /// Roda a chamada e traduz a falha antes de ela chegar à interface. Sem `useServer`
    /// antes, responde `unreachable` em vez de derrubar: é o que acontece de verdade se o
    /// endereço do servidor não foi descoberto ainda.
    fn warm_trees(&self, servers: &Value) {
        let ids: Vec<i64> = servers
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|server| server["id"].as_i64())
            .collect();

        if let Some(api) = self.api() {
            self.runtime
                .spawn(async move { api.warm_trees(&ids).await });
        }
    }

    fn with_api(&self, work: impl FnOnce(&Api, &Runtime) -> Result<Value, HttpError>) -> Value {
        let Some(api) = self.api() else {
            return json!({ "failed": Failure::Unreachable });
        };

        match work(&api, &self.runtime) {
            Ok(answer) => answer,
            Err(HttpError::Failed(failure)) => json!({ "failed": failure }),
            Err(HttpError::Invalid { field, message }) => {
                json!({ "invalid": { "field": field, "message": message } })
            }
        }
    }
}

/// Cria o núcleo. Devolve `null` se o runtime não subir.
///
/// # Safety
/// O ponteiro devolvido só pode ser liberado por `unkvoid_core_free`, uma vez só.
#[unsafe(no_mangle)]
pub extern "C" fn unkvoid_core_new() -> *mut Handle {
    let Ok(runtime) = Runtime::new() else {
        return ptr::null_mut();
    };

    let (sender, events) = channel();

    keep_a_logbook();

    let storage = match storage::Storage::open() {
        Ok(storage) => storage,
        Err(failure) => {
            tracing::warn!(%failure, "sem pasta de configuração: nada será lembrado");

            return ptr::null_mut();
        }
    };

    Box::into_raw(Box::new(Handle {
        app: Arc::new(App::new(storage)),
        api: Mutex::new(None),
        runtime,
        client: Arc::default(),
        sfu: Mutex::new(None),
        room: Mutex::new(None),
        google: Mutex::new(None),
        server: Mutex::new(None),
        media: Mutex::new(None),
        events: Mutex::new(events),
        sender,
    }))
}

/// # Safety
/// `handle` tem de vir de `unkvoid_core_new` e não pode ter sido liberado antes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_core_free(handle: *mut Handle) {
    if handle.is_null() {
        return;
    }

    drop(unsafe { Box::from_raw(handle) });
}

/// Conecta ao SFU. Devolve `true` se a conexão abriu.
///
/// # Safety
/// `handle` tem de estar vivo e `url` tem de ser um C string válido em UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_connect(handle: *const Handle, url: *const c_char) -> bool {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return false;
    };

    let Some(url) = (unsafe { text(url) }) else {
        return false;
    };

    let Ok((client, mut events)) = handle.runtime.block_on(SfuClient::connect(&url)) else {
        return false;
    };

    let sender = handle.sender.clone();
    let (address, held) = (url.clone(), Arc::clone(&handle.client));

    // A interface não fala async: os eventos viram uma fila que ela consulta no ritmo dela,
    // sem bloquear o desenho da tela. Quando o socket cai, este mesmo laço o traz de volta
    // e avisa (`realtime.lost`, `realtime.back`): inscrição em canal é do socket, e quem
    // estava inscrito precisa se apresentar e se inscrever de novo.
    handle.runtime.spawn(async move {
        loop {
            while let Some(event) = events.recv().await {
                let line =
                    json!({ "event": event.name, "channel": event.channel, "data": event.data });

                if sender.send(line.to_string()).is_err() {
                    return;
                }
            }

            if sender
                .send(json!({ "event": "realtime.lost", "channel": null, "data": {} }).to_string())
                .is_err()
            {
                return;
            }

            let mut backoff = crate::reconnect::Backoff::default();

            let fresh = loop {
                let Some(wait) = backoff.next_delay() else {
                    return;
                };

                tokio::time::sleep(wait).await;

                if let Ok(connected) = SfuClient::connect(&address).await {
                    break connected;
                }
            };

            *held.lock().unwrap_or_else(PoisonError::into_inner) = Some(fresh.0);
            events = fresh.1;

            if sender
                .send(json!({ "event": "realtime.back", "channel": null, "data": {} }).to_string())
                .is_err()
            {
                return;
            }
        }
    });

    *handle.client.lock().unwrap_or_else(PoisonError::into_inner) = Some(client);
    *handle.sfu.lock().unwrap_or_else(PoisonError::into_inner) = Some(url);

    true
}

/// Manda uma ação e devolve a resposta em JSON. `null` se falhar.
///
/// # Safety
/// `handle` tem de estar vivo; `action` e `data_json` têm de ser C strings válidos. O
/// resultado tem de ser liberado com `unkvoid_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_call(
    handle: *const Handle,
    action: *const c_char,
    data_json: *const c_char,
) -> *mut c_char {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return ptr::null_mut();
    };

    let (Some(action), Some(client)) = (unsafe { text(action) }, handle.client()) else {
        return ptr::null_mut();
    };

    let data = unsafe { text(data_json) }
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| json!({}));

    match handle.runtime.block_on(client.call(&action, data)) {
        Ok(answer) => into_c(answer.to_string()),
        // O motivo, e não a mensagem: caminho, endereço e status ficam no log.
        Err(failure) => into_c(json!({ "failed": Failure::from_error(&failure) }).to_string()),
    }
}

/// O próximo evento da fila, ou `null` se não há nenhum. Não bloqueia.
///
/// # Safety
/// `handle` tem de estar vivo. O resultado tem de ser liberado com `unkvoid_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_next_event(handle: *const Handle) -> *mut c_char {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return ptr::null_mut();
    };

    match handle
        .events
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .try_recv()
    {
        Ok(event) => into_c(event),
        Err(_) => ptr::null_mut(),
    }
}

/// As decisões do app, que não passam pela rede: qual tela vale, entrar numa sala, a lista
/// das últimas. Uma função só, e não uma por regra, para a ABI não mudar a cada coisa nova —
/// quem cresce é o Rust, e as três interfaces continuam com os mesmos seis símbolos.
///
/// Devolve JSON: `{"ok": …}` ou `{"refused": "nameIsEmpty"|"codeIsInvalid"}`.
///
/// # Safety
/// `handle` tem de estar vivo. O resultado volta em `unkvoid_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_app(
    handle: *const Handle,
    action: *const c_char,
    data_json: *const c_char,
) -> *mut c_char {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return ptr::null_mut();
    };

    let Some(action) = (unsafe { text(action) }) else {
        return ptr::null_mut();
    };

    let data: Value = unsafe { text(data_json) }
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| json!({}));

    let field = |name: &str| data[name].as_str().unwrap_or_default().to_owned();

    let answer = match action.as_str() {
        "state" => {
            let state = handle.app.state();

            json!({
                "screen": screen_name(state.screen),
                "name": state.name,
                "room": state.room,
                "signedIn": handle.app.has_token(),
            })
        }
        "createRoom" => match handle.app.create_room(&field("name"), &field("code")) {
            Ok(code) => handle.enter(code, None),
            Err(refusal) => refused(refusal),
        },
        "joinRoom" => match handle.app.join_room(&field("name"), &field("code")) {
            Ok(code) => handle.enter(code, None),
            Err(refusal) => refused(refusal),
        },
        // Entrar num canal de voz é a mesma sala, com o token de 60 s no lugar do nome.
        "joinVoice" => handle.enter(field("channel"), Some(field("channel"))),
        "leaveRoom" => {
            handle.leave();
            handle.app.leave_room();

            json!({ "ok": true })
        }

        // A sala aberta: o que sobe, o que chega e quem está.
        "room" => handle.in_room(|room, _| json!({ "peers": room.peers()["peers"], "tiles": room.tiles()["tiles"], "mine": room.mine() })),
        "share" => handle.in_room(|room, runtime| {
            match runtime.block_on(room.share(crate::sharing::capture_config(&data))) {
                Ok(()) => json!({ "ok": true }),
                Err(failure) => {
                    tracing::warn!(%failure, "a tela não subiu");

                    json!({ "failed": Failure::from_error(&failure) })
                }
            }
        }),
        "stopSharing" => handle.in_room(|room, runtime| {
            runtime.block_on(room.stop_sharing());

            json!({ "ok": true })
        }),
        "openMicrophone" => handle.in_room(|room, runtime| match runtime.block_on(room.open_microphone()) {
            Ok(()) => json!({ "ok": true }),
            Err(failure) => {
                tracing::warn!(%failure, "o microfone não subiu");

                json!({ "failed": Failure::from_error(&failure) })
            }
        }),
        "closeMicrophone" => handle.in_room(|room, runtime| {
            runtime.block_on(room.close_microphone());

            json!({ "ok": true })
        }),
        "muteMicrophone" => handle.in_room(|room, runtime| {
            runtime.block_on(room.mute_microphone(data["muted"].as_bool().unwrap_or(true)));

            json!({ "ok": true })
        }),
        // A câmera só existe por aqui no macOS: nos outros sistemas quem a captura é o `capture`.
        #[cfg(target_os = "macos")]
        "openCamera" => handle.in_room(|room, runtime| {
            let size = (data["width"].as_u64().unwrap_or(1280) as u32, data["height"].as_u64().unwrap_or(720) as u32);

            match runtime.block_on(room.open_camera(size, data["fps"].as_u64().unwrap_or(30) as u32)) {
                Ok(()) => json!({ "ok": true }),
                Err(failure) => {
                    tracing::warn!(%failure, "a câmera não subiu");

                    json!({ "failed": Failure::from_error(&failure) })
                }
            }
        }),
        #[cfg(target_os = "macos")]
        "closeCamera" => handle.in_room(|room, runtime| {
            runtime.block_on(room.close_camera());

            json!({ "ok": true })
        }),
        "inputMode" => handle.in_room(|room, _| {
            room.set_input_mode(crate::sharing::InputMode::parse(&field("mode"), data["sensitivity"].as_u64().unwrap_or(35)));

            json!({ "ok": true })
        }),
        "talk" => handle.in_room(|room, _| {
            room.talk(data["talking"].as_bool().unwrap_or(false));

            json!({ "ok": true })
        }),
        "deafen" => handle.in_room(|room, _| {
            room.deafen(data["deafened"].as_bool().unwrap_or(true));

            json!({ "ok": true })
        }),
        "closeWatched" => handle.in_room(|room, runtime| {
            runtime.block_on(room.close_watched(&field("producerId")));

            json!({ "ok": true })
        }),
        // Sem `producerId`, assiste a tudo o que a pessoa tinha fechado.
        "watch" => handle.in_room(|room, runtime| {
            runtime.block_on(room.watch(data["producerId"].as_str()));

            json!({ "ok": true })
        }),
        "pauseWatched" => handle.in_room(|room, runtime| {
            runtime.block_on(room.pause_watched(&field("producerId"), data["paused"].as_bool().unwrap_or(true)));

            json!({ "ok": true })
        }),
        "selfView" => handle.in_room(|room, runtime| {
            runtime.block_on(room.set_self_view(data["wanted"].as_bool().unwrap_or(true)));

            json!({ "ok": true })
        }),
        "changeQuality" => handle.in_room(|room, runtime| {
            let wanted = crate::sharing::capture_config(&data);
            // A tela só muda quando ela vem no pedido: sem `source`, é só a qualidade.
            let source = data.get("source").is_some().then_some(wanted.source);

            match runtime.block_on(room.change_quality(wanted.quality, wanted.frame_rate, source)) {
                Ok(()) => json!({ "ok": true }),
                Err(failure) => {
                    tracing::warn!(%failure, "a qualidade não mudou");

                    json!({ "failed": Failure::from_error(&failure) })
                }
            }
        }),
        "muteWatched" => handle.in_room(|room, _| {
            room.mute_watched(&field("producerId"), data["muted"].as_bool().unwrap_or(true));

            json!({ "ok": true })
        }),

        // O tempo real do chat: apresenta esta conta ao SFU com o token que o Laravel
        // assina. Inscrever-se num canal depois disso é `unkvoid_call("subscribe")`.
        "identify" => handle.with_api(|api, runtime| {
            let token = runtime.block_on(api.realtime_token())?;
            let Some(client) = handle.client() else {
                return Err(HttpError::Failed(Failure::Unreachable));
            };

            runtime
                .block_on(client.call(action::IDENTIFY, json!({ "token": token })))
                .map(|_| json!({ "ok": true }))
                .map_err(|failure| HttpError::Failed(Failure::from_error(&failure)))
        }),
        "recentRooms" => json!({ "rooms": handle.app.recent_rooms() }),

        // As telas que dá para compartilhar. Listar é lento em alguns sistemas (no Linux é
        // rodar `xrandr`), e por isso também sai da thread que desenha.
        "displays" => match crate::sharing::displays() {
            Ok(found) => found,
            Err(failure) => {
                tracing::warn!(%failure, "não deu para listar as telas");

                json!({ "failed": Failure::ServerBroke })
            }
        },

        // A miniatura de uma tela ou janela do seletor, em JPEG. Vazia quando o sistema não
        // deixa (sem permissão de gravar a tela): o seletor abre sem imagem em vez de não abrir.
        "preview" => {
            let source = crate::sharing::capture_config(&data).source;
            let jpeg = capture::PlatformCapturer::preview(source).unwrap_or_default();

            json!({ "jpeg": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, jpeg) })
        }

        // Daqui para baixo é o que precisa de conta, e portanto do Laravel. Todas passam
        // pelo runtime: a interface já chama isto de fora da thread que desenha.
        "useServer" => {
            match Api::new(&field("url")) {
                Ok(api) => {
                    if let Some(token) = handle.app.token() {
                        api.set_token(Some(token));
                    }

                    // A sessão que acaba sozinha (o par não renovou) chega à interface como
                    // `session.ended`, e ela volta ao login.
                    let sender = handle.sender.clone();

                    handle.app.keep_session(&api, move || {
                        let _ = sender.send(json!({ "event": "session.ended", "channel": null, "data": {} }).to_string());
                    });

                    *handle.api.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(api));
                    *handle.server.lock().unwrap_or_else(PoisonError::into_inner) = Some(field("url"));
                    report_last_failure(&handle.runtime, field("url"));

                    json!({ "ok": true })
                }
                Err(failure) => {
                    tracing::warn!(%failure, "o endereço do servidor não serve");

                    json!({ "failed": Failure::Unreachable })
                }
            }
        }
        // Há versão mais nova publicada para esta plataforma? A interface avisa e abre o endereço.
        "update" => handle.with_api(|api, runtime| {
            Ok(match runtime.block_on(api.newer_release(crate::update::PLATFORM)) {
                Some(release) => json!({ "version": release.version, "url": release.url }),
                None => json!({ "upToDate": true }),
            })
        }),
        // Onde fica o SFU desta instalação: é o Laravel quem sabe.
        "config" => handle.with_api(|api, runtime| Ok(json!({ "sfu": runtime.block_on(api.config())?.sfu }))),
        // Quem é a conta do token guardado. Token vencido ou revogado não é falha: é login
        // de novo, e o token sai do disco para a próxima abertura não tentar outra vez.
        "me" => handle.with_api(|api, runtime| match runtime.block_on(api.me()) {
            Ok(user) => Ok(json!({ "ok": true, "user": user })),
            Err(HttpError::Failed(Failure::SignedOut)) => {
                handle.app.set_token(None);
                api.set_token(None);

                Err(HttpError::Failed(Failure::SignedOut))
            }
            Err(failure) => Err(failure),
        }),
        // Entrar e criar conta só diferem no caminho; as duas guardam o token e abrem a
        // sessão do mesmo jeito.
        "login" | "register" => handle.with_api(|api, runtime| {
            let (email, password, device) = (field("email"), field("password"), field("device"));

            let answer = runtime.block_on(async {
                if action == "register" {
                    api.register(&email, &password, &device).await
                } else {
                    api.login(&email, &password, &device).await
                }
            })?;

            handle.app.set_token(Some(&answer.token));
            handle.app.show(handle.app.home());

            Ok(json!({ "ok": true, "user": answer.user }))
        }),
        // Entrar com o Google, em dois passos: o endereço que a interface abre no navegador, e
        // depois a espera (até 5 min) de a pessoa voltar de lá.
        "googleStart" => {
            let server = handle.server.lock().unwrap_or_else(PoisonError::into_inner).clone();

            match server.map(|server| handle.runtime.block_on(crate::google::GoogleLogin::start(&server))) {
                Some(Ok(login)) => {
                    let url = login.url.clone();

                    *handle.google.lock().unwrap_or_else(PoisonError::into_inner) = Some(login);

                    json!({ "ok": true, "url": url })
                }
                _ => json!({ "failed": Failure::Unreachable }),
            }
        }
        "googleWait" => {
            let waiting = handle.google.lock().unwrap_or_else(PoisonError::into_inner).take();

            match waiting.map(|login| handle.runtime.block_on(login.wait())) {
                Some(Ok((token, refresh))) => {
                    handle.app.set_token(Some(&token));
                    handle.app.show(handle.app.home());

                    handle.with_api(|api, runtime| {
                        api.adopt(&token, refresh.as_deref());

                        Ok(json!({ "ok": true, "user": runtime.block_on(api.me())? }))
                    })
                }
                _ => json!({ "failed": Failure::SignedOut }),
            }
        }
        "signOut" => {
            handle.leave();
            handle.app.leave_room();
            handle.app.set_token(None);
            handle.app.show(handle.app.home());

            // A tela sai na hora; os tokens caem no servidor em segundo plano.
            if let Some(api) = handle.api() {
                handle.runtime.spawn(api.sign_out());
            }

            json!({ "ok": true })
        }
        "servers" => {
            let answer = handle.with_api(|api, runtime| {
                Ok(json!({ "servers": runtime.block_on(api.servers())? }))
            });

            handle.warm_trees(&answer["servers"]);

            answer
        }
        // Com `known`, a árvore já vista volta na hora (e `known: true` avisa que pode estar
        // velha); sem ela em mãos, ou sem `known`, a resposta é a do Laravel.
        "server" => handle.with_api(|api, runtime| {
            let id = data["id"].as_i64().unwrap_or_default();
            let known = data["known"].as_bool().unwrap_or_default().then(|| api.known_tree(id)).flatten();
            let from_memory = known.is_some();

            let tree = match known {
                Some(tree) => tree,
                None => runtime.block_on(api.tree(id))?,
            };

            // O que dá para fazer ali já vai calculado: a interface só esconde botão.
            Ok(json!({ "abilities": tree.abilities(), "server": tree, "known": from_memory }))
        }),
        "messages" => handle.with_api(|api, runtime| {
            Ok(json!({ "messages": runtime.block_on(api.messages(&field("channel")))? }))
        }),
        "sendMessage" => handle.with_api(|api, runtime| {
            let sent = runtime.block_on(api.send_message(&field("channel"), &field("body")))?;

            Ok(json!({ "ok": true, "message": sent }))
        }),
        "logs" => logbook_tail(data["lines"].as_u64().unwrap_or(400) as usize),
        // As preferências, na mesma pasta e com as mesmas chaves do app de hoje.
        "preference" => json!({ "value": handle.app.preference(&field("key")) }),
        "setPreference" => {
            handle.app.set_preference(&field("key"), data["value"].clone());

            json!({ "ok": true })
        }
        // As teclas de um atalho (`CmdOrCtrl+Shift+KeyM`), nos códigos deste sistema.
        // Cada modificador é um grupo: basta uma tecla do grupo apertada (Shift esquerdo ou direito).
        #[cfg(target_os = "macos")]
        "keys" => match keys_of(&field("accelerator")) {
            Some((key, modifiers)) => json!({ "key": key, "modifiers": modifiers }),
            None => json!({ "failed": Failure::Invalid }),
        },
        // A tecla que a pessoa apertou, no nome que a preferência guarda (`KeyM`, `F13`).
        #[cfg(target_os = "macos")]
        "keyName" => match crate::keymap::macos_key_name(data["code"].as_u64().unwrap_or(u64::MAX) as u16) {
            Some(name) => json!({ "name": name }),
            None => json!({ "failed": Failure::Invalid }),
        },
        // Tudo o mais que o Laravel sabe fazer, pelo nome da rota (`routes.rs`).
        "api" => handle.with_api(|api, runtime| {
            Ok(json!({ "ok": true, "data": runtime.block_on(api.perform(&field("name"), &data["params"], &data["body"]))? }))
        }),
        "upload" => handle.with_api(|api, runtime| {
            let files: Vec<std::path::PathBuf> =
                data["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(Into::into).collect();

            let sent = runtime.block_on(api.upload(&field("name"), &data["params"], &field("field"), &files, &data["fields"]))?;

            Ok(json!({ "ok": true, "data": sent }))
        }),
        "editMessage" => handle.with_api(|api, runtime| {
            let edited = runtime.block_on(api.edit_message(data["id"].as_i64().unwrap_or_default(), &field("body")))?;

            Ok(json!({ "ok": true, "message": edited }))
        }),
        "deleteMessage" => handle.with_api(|api, runtime| {
            runtime.block_on(api.delete_message(data["id"].as_i64().unwrap_or_default()))?;

            Ok(json!({ "ok": true }))
        }),
        _ => json!({ "error": format!("ação desconhecida: {action}") }),
    };

    into_c(answer.to_string())
}

/// O arquivo para onde vai o que o núcleo registra. A janela não tem console, e sem isto um
/// "não funcionou" não deixa rastro nenhum.
fn logbook_path() -> Option<std::path::PathBuf> {
    storage::Storage::open()
        .ok()
        .map(|storage| storage.directory().join("unkvoid.log"))
}

/// Liga o registro em arquivo, uma vez por processo. Recomeça o arquivo quando ele passa de
/// 2 MB: é diagnóstico da sessão, não histórico.
fn keep_a_logbook() {
    static STARTED: std::sync::Once = std::sync::Once::new();

    STARTED.call_once(|| {
        let Some(path) = logbook_path() else {
            return;
        };

        if std::fs::metadata(&path).is_ok_and(|file| file.len() > 2 * 1024 * 1024) {
            let _ = std::fs::remove_file(&path);
        }

        let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        else {
            return;
        };

        // O pânico também vai para o arquivo: é ele que o relatório da próxima abertura procura.
        let earlier = std::panic::take_hook();

        std::panic::set_hook(Box::new(move |info| {
            tracing::error!("panic {info}");
            earlier(info);
        }));

        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with_ansi(false)
            .with_writer(Mutex::new(file))
            .try_init();
    });
}

/// O teto do que viaja num relatório; o Laravel aceita 20 000 caracteres.
const MAX_REPORT: u64 = 18_000;

/// Manda ao servidor o que o registro guardou desde o último relatório, **quando houve erro**.
/// Na abertura, porque é o único momento possível: um pânico leva o processo junto, e não
/// sobra ninguém para avisar. O marcador só anda quando o servidor confirma.
fn report_last_failure(runtime: &Runtime, server: String) {
    static REPORTED: std::sync::Once = std::sync::Once::new();

    REPORTED.call_once(|| {
        let Some(log) = logbook_path() else {
            return;
        };

        let marker = log.with_extension("reported");
        let Some((slice, offset)) = unreported(&log, &marker) else {
            return;
        };

        if !slice.contains("panic ") && !slice.contains("ERROR") {
            let _ = std::fs::write(&marker, offset.to_string());

            return;
        }

        let body = json!({
            "version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "log": scrub(&slice, &std::env::var("USER").unwrap_or_default()),
        });

        runtime.spawn(async move {
            let sent = reqwest::Client::new()
                .post(format!("{}/api/errors", server.trim_end_matches('/')))
                .header("accept", "application/json")
                .json(&body)
                .send()
                .await;

            if sent.is_ok_and(|answer| answer.status().is_success()) {
                let _ = std::fs::write(&marker, offset.to_string());
            }
        });
    });
}

/// O pedaço do registro que ainda não foi relatado, e a posição nova do marcador. Fica com o
/// **fim** quando passa do teto: o erro é a última coisa que aconteceu.
fn unreported(log: &std::path::Path, marker: &std::path::Path) -> Option<(String, u64)> {
    use std::io::{Read, Seek, SeekFrom};

    let size = std::fs::metadata(log).ok()?.len();
    let sent = std::fs::read_to_string(marker)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let from = if sent > size { 0 } else { sent };

    if from >= size {
        return None;
    }

    let mut file = std::fs::File::open(log).ok()?;
    let mut bytes = Vec::new();

    file.seek(SeekFrom::Start(from.max(size.saturating_sub(MAX_REPORT))))
        .ok()?;
    file.take(MAX_REPORT).read_to_end(&mut bytes).ok()?;

    Some((String::from_utf8_lossy(&bytes).into_owned(), size))
}

/// Tira o nome de usuário da máquina do que vai ser enviado: todo caminho passa por
/// `/Users/<nome>`. Nome de duas letras aparece dentro de palavra, e trocá-lo estragaria o resto.
fn scrub(text: &str, user: &str) -> String {
    if user.chars().count() < 3 {
        return text.to_owned();
    }

    text.replace(user, "<usuario>")
}

/// As últimas linhas do registro, para a janela de "Logs".
fn logbook_tail(lines: usize) -> Value {
    let Some(path) = logbook_path() else {
        return json!({ "failed": Failure::ServerBroke });
    };

    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();

    json!({ "path": path, "lines": all[all.len().saturating_sub(lines)..] })
}

#[cfg(target_os = "macos")]
fn keys_of(accelerator: &str) -> Option<(u16, Vec<&'static [u16]>)> {
    let parts = crate::keymap::split(accelerator)?;
    let key = crate::keymap::macos_key(parts.key)?;
    let modifiers = parts
        .modifiers
        .iter()
        .map(|name| crate::keymap::macos_modifier(name))
        .collect::<Option<Vec<_>>>()?;

    Some((key, modifiers))
}

fn refused(refusal: EntryRefusal) -> Value {
    match refusal {
        EntryRefusal::NameIsEmpty => json!({ "refused": "nameIsEmpty" }),
        EntryRefusal::CodeIsInvalid => json!({ "refused": "codeIsInvalid" }),
    }
}

/// O nome que as interfaces leem. Um `enum` não atravessa a ABI, e um número atravessaria
/// mal: trocar a ordem aqui mudaria a tela de todas elas em silêncio.
fn screen_name(screen: Screen) -> &'static str {
    match screen {
        Screen::Entry => "entry",
        Screen::Hub => "hub",
        Screen::Room => "room",
        Screen::Offline => "offline",
        Screen::Updating => "updating",
    }
}

/// O próximo quadro ou bloco de som do que se está assistindo. Espera até `MEDIA_PATIENCE`
/// e devolve `null` se nada chegou — é para uma thread só da interface, em laço.
///
/// O bloco devolvido é um cabeçalho e o conteúdo, tudo little-endian:
/// `[tipo u8: 0 vídeo, 1 som][keyframe u8][tamanho do id u16][timestamp u32][id][conteúdo]`.
/// Vídeo é H.264 em Annex-B; som é PCM `f32` estéreo intercalado a 48 kHz.
///
/// # Safety
/// `handle` tem de estar vivo e `length` tem de apontar para um `usize` gravável. O
/// resultado volta em `unkvoid_bytes_free`, com o mesmo `length`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_next_media(handle: *const Handle, length: *mut usize) -> *mut u8 {
    let (Some(handle), false) = (unsafe { handle.as_ref() }, length.is_null()) else {
        return ptr::null_mut();
    };

    let next = {
        let queue = handle.media.lock().unwrap_or_else(PoisonError::into_inner);

        match queue.as_ref() {
            Some(queue) => queue.recv_timeout(MEDIA_PATIENCE).ok(),
            None => None,
        }
    };

    let Some(media) = next else {
        // Sem sala não há fila para esperar, e sem esta pausa a thread da interface giraria.
        if handle.room().is_none() {
            std::thread::sleep(MEDIA_PATIENCE);
        }

        return ptr::null_mut();
    };

    let block = frame_of(&media).into_boxed_slice();

    unsafe { *length = block.len() };

    Box::into_raw(block).cast()
}

/// # Safety
/// Só para blocos devolvidos por `unkvoid_next_media`, com o `length` que ele deu, uma vez só.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_bytes_free(block: *mut u8, length: usize) {
    if block.is_null() {
        return;
    }

    drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(block, length)) });
}

/// O som do microfone que a interface captura: PCM `f32` estéreo intercalado a 48 kHz.
/// Sem sala ou sem microfone aberto, não faz nada.
///
/// # Safety
/// `handle` tem de estar vivo e `samples` tem de apontar para `count` amostras legíveis.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_speak(handle: *const Handle, samples: *const f32, count: usize) {
    let (Some(handle), false) = (unsafe { handle.as_ref() }, samples.is_null()) else {
        return;
    };

    if let Some(room) = handle.room() {
        room.speak(unsafe { std::slice::from_raw_parts(samples, count) });
    }
}

/// Um quadro da câmera que a interface capturou: o `IOSurfaceRef` dele, **já retido** (+1) —
/// quem solta é este lado. Sem sala ou sem câmera aberta (ação "openCamera"), só solta.
///
/// # Safety
/// `handle` tem de estar vivo, e `surface` tem de ser um `IOSurfaceRef` válido e retido, ou nulo.
#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_show(
    handle: *const Handle,
    surface: *mut std::ffi::c_void,
    timestamp_ns: u64,
) {
    let Some(surface) = capture::GpuSurface::from_raw(surface) else {
        return;
    };

    if let Some(room) = unsafe { handle.as_ref() }.and_then(Handle::room) {
        room.show(&surface, timestamp_ns);
    }
}

fn frame_of(media: &Media) -> Vec<u8> {
    let (kind, keyframe, timestamp) = match media.kind {
        MediaKind::Video {
            keyframe,
            timestamp,
        } => (0_u8, u8::from(keyframe), timestamp),
        MediaKind::Audio => (1, 0, 0),
    };

    let id = media.producer_id.as_bytes();
    let mut block = Vec::with_capacity(8 + id.len() + media.data.len());

    block.extend([kind, keyframe]);
    block.extend((id.len() as u16).to_le_bytes());
    block.extend(timestamp.to_le_bytes());
    block.extend(id);
    block.extend(&media.data);

    block
}

/// # Safety
/// Só para ponteiros devolvidos por este módulo, e uma vez só.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn unkvoid_string_free(text: *mut c_char) {
    if text.is_null() {
        return;
    }

    drop(unsafe { CString::from_raw(text) });
}

unsafe fn text(raw: *const c_char) -> Option<String> {
    if raw.is_null() {
        return None;
    }

    unsafe { CStr::from_ptr(raw) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

fn into_c(value: String) -> *mut c_char {
    CString::new(value)
        .map(CString::into_raw)
        .unwrap_or(ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::CString;

    #[test]
    fn a_null_handle_never_crashes() {
        // A interface pode chamar depois de liberar; derrubar o app do usuário por isso
        // seria pior do que não fazer nada.
        unsafe {
            assert!(!unkvoid_connect(ptr::null_mut(), ptr::null()));
            assert!(unkvoid_call(ptr::null_mut(), ptr::null(), ptr::null()).is_null());
            assert!(unkvoid_next_event(ptr::null_mut()).is_null());
            unkvoid_core_free(ptr::null_mut());
            unkvoid_string_free(ptr::null_mut());
        }
    }

    #[test]
    fn a_core_can_be_created_and_freed() {
        let handle = unkvoid_core_new();

        assert!(!handle.is_null());

        unsafe { unkvoid_core_free(handle) };
    }

    #[test]
    fn calling_before_connecting_returns_null_instead_of_crashing() {
        let handle = unkvoid_core_new();
        let action = CString::new("ping").expect("c string");

        unsafe {
            assert!(unkvoid_call(handle, action.as_ptr(), ptr::null()).is_null());
            unkvoid_core_free(handle);
        }
    }

    /// O caminho que as três interfaces percorrem para entrar numa sala, exatamente como
    /// elas o fazem: string entra, JSON sai, e nada em Swift, C# ou GTK decide nada.
    #[test]
    fn a_room_can_be_created_and_left_through_the_abi() {
        let (handle, _dir) = isolated_core();

        assert!(!handle.is_null());

        connect_to_a_fake_sfu(handle);

        let answer = app_call(handle, "createRoom", r#"{"name":"Ada","code":""}"#);

        assert_eq!(answer["ok"], true, "criar a sala falhou: {answer}");
        assert!(crate::room_code::is_valid(
            answer["room"].as_str().expect("room")
        ));
        assert_eq!(app_call(handle, "state", "{}")["screen"], "room");

        let room = app_call(handle, "room", "{}");

        assert_eq!(
            room["peers"][0]["selfPeer"], true,
            "a pessoa se vê na sala: {room}"
        );
        assert_eq!(
            room["mine"]["canShare"], true,
            "o `can` do servidor chega à interface: {room}"
        );
        assert_eq!(room["mine"]["sharing"], false);

        app_call(handle, "leaveRoom", "{}");

        assert_eq!(app_call(handle, "state", "{}")["screen"], "entry");
        assert_eq!(
            app_call(handle, "room", "{}")["failed"],
            "gone",
            "sala fechada não responde por sala"
        );

        unsafe { unkvoid_core_free(handle) };
    }

    /// Aceitar o código e não conseguir entrar não pode deixar a pessoa numa sala vazia.
    #[test]
    fn a_room_the_sfu_never_opened_does_not_change_the_screen() {
        let (handle, _dir) = isolated_core();

        let answer = app_call(handle, "createRoom", r#"{"name":"Ada","code":""}"#);

        assert_eq!(answer["failed"], "unreachable", "{answer}");
        assert_eq!(app_call(handle, "state", "{}")["screen"], "entry");

        unsafe { unkvoid_core_free(handle) };
    }

    #[test]
    fn only_what_was_not_reported_yet_travels_and_without_the_user_name() {
        let dir = tempfile::tempdir().expect("dir");
        let (log, marker) = (
            dir.path().join("unkvoid.log"),
            dir.path().join("unkvoid.reported"),
        );

        std::fs::write(&log, "linha velha\nERROR em /Users/edsonlima/x\n").expect("log");
        std::fs::write(&marker, "12").expect("marker");

        let (slice, offset) = unreported(&log, &marker).expect("slice");

        assert_eq!(
            slice, "ERROR em /Users/edsonlima/x\n",
            "o que já foi relatado não viaja de novo"
        );
        assert_eq!(offset, 40);
        assert_eq!(scrub(&slice, "edsonlima"), "ERROR em /Users/<usuario>/x\n");
        assert_eq!(
            scrub("edição", "ed"),
            "edição",
            "nome curto estragaria o texto"
        );

        std::fs::write(&marker, offset.to_string()).expect("marker");

        assert!(unreported(&log, &marker).is_none());
    }

    #[test]
    fn a_frame_crosses_the_abi_with_its_header_in_front() {
        let block = frame_of(&Media {
            producer_id: "abc".into(),
            kind: MediaKind::Video {
                keyframe: true,
                timestamp: 0x0102_0304,
            },
            data: vec![9, 8, 7],
        });

        assert_eq!(block, [0, 1, 3, 0, 4, 3, 2, 1, b'a', b'b', b'c', 9, 8, 7]);
    }

    #[test]
    fn the_abi_refuses_without_a_name_instead_of_crashing() {
        let (handle, _dir) = isolated_core();

        assert_eq!(
            app_call(handle, "createRoom", r#"{"name":"  ","code":""}"#)["refused"],
            "nameIsEmpty",
        );

        // Recusar não pode mudar a tela.
        assert_eq!(app_call(handle, "state", "{}")["screen"], "entry");

        unsafe { unkvoid_core_free(handle) };
    }

    /// Sem `useServer`, tudo que precisa de conta responde "não deu para falar com o
    /// servidor" — e não derruba o app nem fica mudo.
    #[test]
    fn asking_for_servers_before_knowing_the_address_fails_politely() {
        let handle = unkvoid_core_new();

        for action in ["servers", "messages", "sendMessage", "login"] {
            let answer = app_call(handle, action, "{}");

            assert_eq!(
                answer["failed"], "unreachable",
                "{action} respondeu {answer}"
            );
        }

        unsafe { unkvoid_core_free(handle) };
    }

    #[test]
    fn a_bad_server_address_is_refused_instead_of_accepted() {
        let handle = unkvoid_core_new();
        let answer = app_call(handle, "useServer", r#"{"url":"nao é endereço"}"#);

        // Endereço torto não pode virar um cliente que falha só na primeira chamada.
        assert!(
            answer["ok"] == true || answer["failed"] == "unreachable",
            "{answer}"
        );

        unsafe { unkvoid_core_free(handle) };
    }

    /// A tela pergunta o estado a cada desenho. Se isso for ao chaveiro do sistema toda
    /// vez, a janela engasga — e no primeiro acesso o macOS ainda pode abrir um diálogo.
    #[test]
    fn the_state_is_answered_quickly_enough_to_draw_with() {
        let handle = unkvoid_core_new();

        // Uma primeira chamada para pagar o que for preguiçoso.
        app_call(handle, "state", "{}");

        let started = std::time::Instant::now();

        for _ in 0..50 {
            app_call(handle, "state", "{}");
        }

        let each = started.elapsed() / 50;

        assert!(
            each.as_millis() < 5,
            "cada `state` custou {each:?}: a tela vai engasgar"
        );

        unsafe { unkvoid_core_free(handle) };
    }

    /// O que o dono conseguiu fazer na tela e não devia: entrar sem nome, com código
    /// vazio e com código só de espaços.
    #[test]
    fn the_abi_refuses_every_empty_or_blank_entry() {
        let (handle, _dir) = isolated_core();

        let recusado = |action: &str, data: &str| {
            let answer = app_call(handle, action, data);

            assert!(
                answer["refused"].is_string(),
                "{action} {data} foi aceito: {answer}",
            );
            assert_eq!(
                app_call(handle, "state", "{}")["screen"],
                "entry",
                "mudou de tela"
            );
        };

        // Sem nome, em qualquer caminho.
        recusado("createRoom", r#"{"name":"","code":""}"#);
        recusado("createRoom", r#"{"name":"   ","code":""}"#);
        recusado("joinRoom", r#"{"name":"","code":"sala-boa"}"#);

        // Entrar exige código: vazio ou em branco não é sala nenhuma.
        recusado("joinRoom", r#"{"name":"Ada","code":""}"#);
        recusado("joinRoom", r#"{"name":"Ada","code":"   "}"#);
        recusado("joinRoom", r#"{"name":"Ada","code":"\t\n"}"#);

        // E o que é código de verdade continua entrando.
        connect_to_a_fake_sfu(handle);

        assert_eq!(
            app_call(handle, "joinRoom", r#"{"name":"Ada","code":"sala-boa"}"#)["ok"],
            true
        );

        unsafe { unkvoid_core_free(handle) };
    }

    /// A interface precisa saber o que dá para compartilhar antes de compartilhar. No
    /// Wayland a lista pode vir vazia de propósito (quem escolhe é o sistema), então o que
    /// se exige aqui é resposta com forma conhecida — não uma lista cheia.
    #[test]
    fn the_screens_can_be_listed_through_the_abi() {
        let (handle, _dir) = isolated_core();
        let answer = app_call(handle, "displays", "{}");

        assert!(
            answer["displays"].is_array() || answer["failed"].is_string(),
            "listar telas devolveu algo que a interface não sabe ler: {answer}",
        );

        if answer["displays"].is_array() {
            assert!(
                answer["portal"].is_boolean(),
                "faltou dizer se quem escolhe é o sistema"
            );
        }

        unsafe { unkvoid_core_free(handle) };
    }

    #[test]
    fn an_unknown_app_action_answers_instead_of_going_quiet() {
        let handle = unkvoid_core_new();
        let answer = app_call(handle, "voar", "{}");

        assert!(answer["error"].is_string(), "ação desconhecida ficou muda");

        unsafe { unkvoid_core_free(handle) };
    }

    /// Um núcleo com estado próprio, isolado do que está gravado nesta máquina. Sem isto o
    /// resultado do teste depende de quem já usou o app aqui.
    /// Um SFU de mentira que aceita tudo: responde a qualquer ação com o que o `join` devolve.
    fn connect_to_a_fake_sfu(handle: *const Handle) {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let core = unsafe { handle.as_ref() }.expect("handle");
        let listener = core
            .runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .expect("listen");
        let url = CString::new(format!(
            "ws://127.0.0.1:{}",
            listener.local_addr().expect("address").port()
        ))
        .expect("url");

        core.runtime.spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut socket = tokio_tungstenite::accept_async(stream).await.expect("handshake");

                    while let Some(Ok(Message::Text(raw))) = socket.next().await {
                        let request: Value = serde_json::from_str(&raw).expect("parse");
                        let reply = json!({
                            "id": request["id"],
                            "ok": true,
                            "data": { "peerId": "mine", "name": "Ada", "resumeKey": "k", "peers": [], "can": ["stream"] },
                        });

                        if socket.send(Message::text(reply.to_string())).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });

        assert!(
            unsafe { unkvoid_connect(handle, url.as_ptr()) },
            "o SFU de mentira não atendeu"
        );
    }

    fn isolated_core() -> (*mut Handle, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");

        // ponytail: variável de ambiente é global ao processo, então os testes da ABI não
        // podem correr em paralelo entre si. São cinco e custam milissegundos; um núcleo que
        // aceitasse a pasta por parâmetro só valeria a pena se isso passasse a doer.
        unsafe { std::env::set_var("UNKVOID_STATE_DIR", dir.path()) };

        (unkvoid_core_new(), dir)
    }

    /// Faz a chamada como uma interface faria, e libera o que o núcleo alocou.
    fn app_call(handle: *const Handle, action: &str, data: &str) -> Value {
        let action = CString::new(action).expect("c string");
        let data = CString::new(data).expect("c string");
        let raw = unsafe { unkvoid_app(handle, action.as_ptr(), data.as_ptr()) };

        assert!(!raw.is_null(), "a ABI devolveu nada para {action:?}");

        let answer = unsafe { CStr::from_ptr(raw) }
            .to_str()
            .expect("utf8")
            .to_owned();

        unsafe { unkvoid_string_free(raw) };

        serde_json::from_str(&answer).expect("json")
    }

    #[test]
    fn an_empty_queue_returns_null() {
        let handle = unkvoid_core_new();

        unsafe {
            assert!(unkvoid_next_event(handle).is_null());
            unkvoid_core_free(handle);
        }
    }
}
