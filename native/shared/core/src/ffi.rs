//! A ponte para as interfaces que não são Rust: Swift no macOS, C# no Windows.
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

use serde_json::{Value, json};
use tokio::runtime::Runtime;

use crate::api::{Api, HttpError};
use crate::app::{App, EntryRefusal};
use crate::client::SfuClient;
use crate::failure::Failure;
use crate::models::Screen;

/// O que a interface segura entre uma chamada e outra.
///
/// Tudo o que muda vive atrás de cadeado, e as funções tomam o handle por `&` e nunca por
/// `&mut`. A interface consulta os eventos num timer **enquanto** uma ação está em voo —
/// é a topologia que ela precisa ter para não travar a tela —, e com `&mut` isso seria
/// corrida de dados: comportamento indefinido, do tipo que derruba o app sem padrão.
pub struct Handle {
    app: App,
    api: Mutex<Option<Arc<Api>>>,
    runtime: Runtime,
    client: Mutex<Option<Arc<SfuClient>>>,
    events: Mutex<Receiver<String>>,
    sender: Sender<String>,
}

impl Handle {
    fn client(&self) -> Option<Arc<SfuClient>> {
        self.client.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn api(&self) -> Option<Arc<Api>> {
        self.api.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Roda a chamada e traduz a falha antes de ela chegar à interface. Sem `useServer`
    /// antes, responde `unreachable` em vez de derrubar: é o que acontece de verdade se o
    /// endereço do servidor não foi descoberto ainda.
    fn with_api(
        &self,
        work: impl FnOnce(&Api, &Runtime) -> Result<Value, HttpError>,
    ) -> Value {
        let Some(api) = self.api() else {
            return json!({ "failed": Failure::Unreachable });
        };

        match work(&api, &self.runtime) {
            Ok(answer) => answer,
            Err(HttpError::Failed(failure)) => json!({ "failed": failure }),
            Err(HttpError::Invalid { field, message }) => json!({ "invalid": { "field": field, "message": message } }),
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

    let storage = match storage::Storage::open() {
        Ok(storage) => storage,
        Err(failure) => {
            tracing::warn!(%failure, "sem pasta de configuração: nada será lembrado");

            return ptr::null_mut();
        }
    };

    Box::into_raw(Box::new(Handle {
        app: App::new(storage),
        api: Mutex::new(None),
        runtime,
        client: Mutex::new(None),
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

    // A interface não fala async: os eventos viram uma fila que ela consulta no ritmo dela,
    // sem bloquear o desenho da tela.
    handle.runtime.spawn(async move {
        while let Some(event) = events.recv().await {
            let line = json!({
                "event": event.name,
                "channel": event.channel,
                "data": event.data,
            });

            if sender.send(line.to_string()).is_err() {
                break;
            }
        }
    });

    *handle.client.lock().unwrap_or_else(PoisonError::into_inner) = Some(client);

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

    match handle.events.lock().unwrap_or_else(PoisonError::into_inner).try_recv() {
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
        "createRoom" => entered(handle.app.create_room(&field("name"), &field("code"))),
        "joinRoom" => entered(handle.app.join_room(&field("name"), &field("code"))),
        "leaveRoom" => {
            handle.app.leave_room();

            json!({ "ok": true })
        }
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

        // Daqui para baixo é o que precisa de conta, e portanto do Laravel. Todas passam
        // pelo runtime: a interface já chama isto de fora da thread que desenha.
        "useServer" => {
            match Api::new(&field("url")) {
                Ok(api) => {
                    if let Some(token) = handle.app.token() {
                        api.set_token(Some(token));
                    }

                    *handle.api.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(api));

                    json!({ "ok": true })
                }
                Err(failure) => {
                    tracing::warn!(%failure, "o endereço do servidor não serve");

                    json!({ "failed": Failure::Unreachable })
                }
            }
        }
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

            Ok(json!({ "ok": true, "user": answer.user }))
        }),
        "signOut" => {
            handle.app.set_token(None);

            if let Some(api) = handle.api() {
                api.set_token(None);
            }

            json!({ "ok": true })
        }
        "servers" => handle.with_api(|api, runtime| {
            Ok(json!({ "servers": runtime.block_on(api.servers())? }))
        }),
        "server" => handle.with_api(|api, runtime| {
            let id = data["id"].as_i64().unwrap_or_default();

            Ok(json!({ "server": runtime.block_on(api.tree(id))? }))
        }),
        "messages" => handle.with_api(|api, runtime| {
            Ok(json!({ "messages": runtime.block_on(api.messages(&field("channel")))? }))
        }),
        "sendMessage" => handle.with_api(|api, runtime| {
            let sent = runtime.block_on(api.send_message(&field("channel"), &field("body")))?;

            Ok(json!({ "ok": true, "message": sent }))
        }),
        _ => json!({ "error": format!("ação desconhecida: {action}") }),
    };

    into_c(answer.to_string())
}

fn entered(result: Result<String, EntryRefusal>) -> Value {
    match result {
        Ok(code) => json!({ "ok": true, "room": code }),
        Err(EntryRefusal::NameIsEmpty) => json!({ "refused": "nameIsEmpty" }),
        Err(EntryRefusal::CodeIsInvalid) => json!({ "refused": "codeIsInvalid" }),
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

    unsafe { CStr::from_ptr(raw) }.to_str().ok().map(str::to_owned)
}

fn into_c(value: String) -> *mut c_char {
    CString::new(value).map(CString::into_raw).unwrap_or(ptr::null_mut())
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

        let answer = app_call(handle, "createRoom", r#"{"name":"Ada","code":""}"#);

        assert_eq!(answer["ok"], true, "criar a sala falhou: {answer}");
        assert!(crate::room_code::is_valid(answer["room"].as_str().expect("room")));
        assert_eq!(app_call(handle, "state", "{}")["screen"], "room");

        app_call(handle, "leaveRoom", "{}");

        assert_eq!(app_call(handle, "state", "{}")["screen"], "entry");

        unsafe { unkvoid_core_free(handle) };
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

            assert_eq!(answer["failed"], "unreachable", "{action} respondeu {answer}");
        }

        unsafe { unkvoid_core_free(handle) };
    }

    #[test]
    fn a_bad_server_address_is_refused_instead_of_accepted() {
        let handle = unkvoid_core_new();
        let answer = app_call(handle, "useServer", r#"{"url":"nao é endereço"}"#);

        // Endereço torto não pode virar um cliente que falha só na primeira chamada.
        assert!(answer["ok"] == true || answer["failed"] == "unreachable", "{answer}");

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

        assert!(each.as_millis() < 5, "cada `state` custou {each:?}: a tela vai engasgar");

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
            assert_eq!(app_call(handle, "state", "{}")["screen"], "entry", "mudou de tela");
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
        assert_eq!(app_call(handle, "joinRoom", r#"{"name":"Ada","code":"sala-boa"}"#)["ok"], true);

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
            assert!(answer["portal"].is_boolean(), "faltou dizer se quem escolhe é o sistema");
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

        let answer = unsafe { CStr::from_ptr(raw) }.to_str().expect("utf8").to_owned();

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
