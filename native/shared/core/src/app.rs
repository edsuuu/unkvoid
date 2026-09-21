//! O estado que as três interfaces desenham, e as decisões que nenhuma delas toma sozinha.
//!
//! Entrar numa sala parece coisa de tela — mas envolve validar o código, guardar o nome, a
//! sala e a lista das recentes. Escrito três vezes, seriam três regras que divergem no
//! primeiro ajuste. Aqui é uma.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use storage::{Cipher, Storage};

use crate::models::Screen;
use crate::room_code;

const NAME_KEY: &str = "unkvoid:name";

/// Os nomes são os que o app de hoje já gravou na máquina de quem usa. Trocar um deles
/// faz a pessoa perder o que estava guardado — e `unkvoid.instalacao`, apesar do
/// português, é a identidade desta instalação perante o SFU: renomear a transforma numa
/// instalação nova.
const ROOM_KEY: &str = "unkvoid:last-room";

const RECENT_KEY: &str = "unkvoid:recent-rooms";

const TOKEN_KEY: &str = "unkvoid:token";

const INSTALL_KEY: &str = "unkvoid.instalacao";

const MAX_RECENT: usize = 8;

/// O motivo de a entrada ter sido recusada. A interface é quem escreve a frase: o texto
/// que a pessoa lê é da interface, e traduzi-lo aqui obrigaria as três a concordar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryRefusal {
    NameIsEmpty,
    CodeIsInvalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub screen: Screen,
    pub name: String,
    pub room: Option<String>,
}

pub struct App {
    state: Mutex<AppState>,
    storage: Storage,
    /// O chaveiro só é aberto quando alguém pede o token. Abrir na partida prenderia o app
    /// na primeira tela em máquina sem Secret Service — e quem nunca fez login não precisa
    /// de chave nenhuma.
    cipher: OnceLock<Option<Cipher>>,
    /// Há conta nesta sessão. Guardado aqui, e não deduzido do disco, porque máquina sem
    /// Secret Service não grava o token: quem acabou de entrar continuaria caindo na tela
    /// do código a cada tela desenhada.
    signed_in: AtomicBool,
}

impl App {
    pub fn new(storage: Storage) -> Self {
        let name = storage.get_string(NAME_KEY).unwrap_or_default();

        let signed_in = AtomicBool::new(storage.get(TOKEN_KEY).is_some());

        Self {
            state: Mutex::new(AppState {
                screen: Screen::home(signed_in.load(Ordering::Relaxed)),
                name,
                room: None,
            }),
            storage,
            cipher: OnceLock::new(),
            signed_in,
        }
    }

    pub fn state(&self) -> AppState {
        self.lock().clone()
    }

    /// Sem código digitado, sorteia um. É o "criar uma sala" da tela de entrada.
    pub fn create_room(&self, name: &str, typed: &str) -> Result<String, EntryRefusal> {
        let typed = typed.trim().to_lowercase();
        let code = if typed.is_empty() { room_code::generate() } else { typed };

        self.open_room(name, &code)
    }

    pub fn join_room(&self, name: &str, typed: &str) -> Result<String, EntryRefusal> {
        self.open_room(name, &typed.trim().to_lowercase())
    }

    fn open_room(&self, name: &str, code: &str) -> Result<String, EntryRefusal> {
        let name = name.trim();

        if name.is_empty() {
            return Err(EntryRefusal::NameIsEmpty);
        }

        if !room_code::is_valid(code) {
            return Err(EntryRefusal::CodeIsInvalid);
        }

        self.remember(name, code);

        let mut state = self.lock();

        state.name = name.to_owned();
        state.room = Some(code.to_owned());
        state.screen = Screen::Room;

        Ok(code.to_owned())
    }

    /// Gravar não pode impedir de entrar: perder a lista de recentes é um incômodo, ficar
    /// de fora da sala é o produto não funcionando.
    fn remember(&self, name: &str, code: &str) {
        let mut recent = self.recent_rooms();

        recent.retain(|known| known != code);
        recent.insert(0, code.to_owned());
        recent.truncate(MAX_RECENT);

        for (key, value) in [(NAME_KEY, name.to_owned()), (ROOM_KEY, code.to_owned())] {
            if let Err(failure) = self.storage.set(key, serde_json::Value::String(value)) {
                tracing::warn!(%failure, key, "não deu para guardar");
            }
        }

        if let Err(failure) = self.storage.set(RECENT_KEY, serde_json::json!(recent)) {
            tracing::warn!(%failure, "não deu para guardar as salas recentes");
        }
    }

    pub fn recent_rooms(&self) -> Vec<String> {
        let Some(saved) = self.storage.get(RECENT_KEY) else {
            return Vec::new();
        };

        let Some(list) = saved.as_array() else {
            return Vec::new();
        };

        // Filtra na leitura: um código inválido gravado por uma versão antiga não pode
        // virar um botão que leva a lugar nenhum.
        list.iter()
            .filter_map(|entry| entry.as_str())
            .filter(|code| room_code::is_valid(code))
            .map(str::to_owned)
            .collect()
    }

    /// A chave desta instalação: identifica este computador na sala sem conta, e nada mais.
    ///
    /// É um código de sala: são os mesmos 36^12 sorteios, e evita uma dependência a mais só
    /// para gerar UUID. Trocar de chave derrubaria a própria sessão anterior na sala, então
    /// ela é sorteada uma vez e guardada.
    pub fn install_id(&self) -> String {
        if let Some(saved) = self.storage.get_string(INSTALL_KEY) {
            return saved;
        }

        let fresh = room_code::generate();

        if let Err(failure) =
            self.storage.set(INSTALL_KEY, serde_json::Value::String(fresh.clone()))
        {
            tracing::warn!(%failure, "não deu para guardar o id da instalação");
        }

        fresh
    }

    /// Há conta nesta sessão? **Não** abre o chaveiro nem lê o disco.
    ///
    /// A tela pergunta isto a cada desenho, e o primeiro acesso ao chaveiro do sistema custa
    /// segundos (no macOS ainda pode abrir um diálogo). Decifrar só quando o token vai ser
    /// usado de verdade é a diferença entre a janela abrir na hora e ela engasgar.
    pub fn has_token(&self) -> bool {
        self.signed_in.load(Ordering::Relaxed)
    }

    /// Onde se cai ao sair de uma sala e ao abrir o app. Quem tem conta volta para os
    /// servidores; quem não tem volta para o código.
    pub fn home(&self) -> Screen {
        Screen::home(self.has_token())
    }

    /// O token do Sanctum, decifrado. Sem chaveiro não há token guardado, e o app pede
    /// login de novo — que é melhor do que deixá-lo legível no disco.
    pub fn token(&self) -> Option<String> {
        self.storage.get_secret(TOKEN_KEY, self.cipher()?)
    }

    pub fn set_token(&self, token: Option<&str>) {
        self.signed_in.store(token.is_some(), Ordering::Relaxed);

        let Some(token) = token else {
            if let Err(failure) = self.storage.remove(TOKEN_KEY) {
                tracing::warn!(%failure, "não deu para apagar o token");
            }

            return;
        };

        let Some(cipher) = self.cipher() else {
            tracing::warn!("sem chaveiro do sistema: o login não fica guardado");

            return;
        };

        if let Err(failure) = self.storage.set_secret(TOKEN_KEY, token, cipher) {
            tracing::warn!(%failure, "não deu para guardar o token");
        }
    }

    fn cipher(&self) -> Option<&Cipher> {
        self.cipher
            .get_or_init(|| match Cipher::open_for(self.storage.directory()) {
                Ok(cipher) => Some(cipher),
                Err(failure) => {
                    tracing::warn!(%failure, "o chaveiro do sistema não abriu");

                    None
                }
            })
            .as_ref()
    }

    pub fn leave_room(&self) {
        let mut state = self.lock();

        state.room = None;
        state.screen = Screen::home(self.signed_in.load(Ordering::Relaxed));
    }

    pub fn show(&self, screen: Screen) {
        self.lock().screen = screen;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AppState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage = Storage::open_at(dir.path()).expect("open");

        (App::new(storage), dir)
    }

    #[test]
    fn creating_without_a_code_draws_one() {
        let (app, _dir) = app();
        let code = app.create_room("Ada", "").expect("create");

        assert!(room_code::is_valid(&code));
        assert_eq!(app.state().screen, Screen::Room);
        assert_eq!(app.state().room.as_deref(), Some(code.as_str()));
    }

    #[test]
    fn a_typed_code_is_normalised_before_use() {
        let (app, _dir) = app();

        assert_eq!(app.join_room("Ada", "  SALA-DO-EDSU  ").expect("join"), "sala-do-edsu");
    }

    #[test]
    fn entering_without_a_name_is_refused() {
        let (app, _dir) = app();

        assert_eq!(app.create_room("   ", "").unwrap_err(), EntryRefusal::NameIsEmpty);
        assert_eq!(app.state().screen, Screen::Entry, "mudou de tela mesmo recusando");
    }

    #[test]
    fn a_malformed_code_is_refused() {
        let (app, _dir) = app();

        assert_eq!(app.join_room("Ada", "-x-").unwrap_err(), EntryRefusal::CodeIsInvalid);
        assert!(app.state().room.is_none());
    }

    #[test]
    fn the_recent_list_puts_the_last_one_first_and_never_repeats() {
        let (app, _dir) = app();

        for code in ["sala-um", "sala-dois", "sala-um"] {
            app.join_room("Ada", code).expect("join");
        }

        assert_eq!(app.recent_rooms(), vec!["sala-um", "sala-dois"]);
    }

    #[test]
    fn the_recent_list_is_capped() {
        let (app, _dir) = app();

        for index in 0..(MAX_RECENT + 5) {
            app.join_room("Ada", &format!("sala-{index}")).expect("join");
        }

        assert_eq!(app.recent_rooms().len(), MAX_RECENT);
    }

    #[test]
    fn an_invalid_code_saved_by_an_older_version_is_ignored() {
        let (app, _dir) = app();

        app.storage.set(RECENT_KEY, serde_json::json!(["boa", "-ruim-", 42])).expect("write");

        assert_eq!(app.recent_rooms(), vec!["boa"]);
    }

    #[test]
    fn leaving_a_room_lands_where_the_account_says() {
        let (app, _dir) = app();

        app.create_room("Ada", "sala-um").expect("create");
        app.leave_room();

        assert_eq!(app.state().screen, Screen::Entry);

        app.set_token(Some("1|abc"));
        app.create_room("Ada", "sala-um").expect("create");
        app.leave_room();

        assert_eq!(app.state().screen, Screen::Hub, "quem tem conta volta para os servidores");
    }

    #[test]
    fn a_machine_without_a_keyring_still_knows_there_is_an_account() {
        let (app, _dir) = app();

        app.set_token(Some("1|abc"));

        assert!(app.has_token());
        assert_eq!(app.home(), Screen::Hub);

        app.set_token(None);

        assert!(!app.has_token());
        assert_eq!(app.home(), Screen::Entry);
    }

    #[test]
    fn the_install_id_survives_a_restart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = App::new(Storage::open_at(dir.path()).expect("open")).install_id();
        let again = App::new(Storage::open_at(dir.path()).expect("reopen")).install_id();

        assert_eq!(first, again, "trocar de id derrubaria a própria sessão anterior na sala");
        assert!(room_code::is_valid(&first));
    }

    /// As chaves não podem divergir do que o app de hoje gravou: quem trocar de app perde
    /// o que estava guardado, e o id da instalação vira outro perante o servidor.
    #[test]
    fn the_keys_are_the_ones_the_app_already_wrote() {
        assert_eq!(NAME_KEY, "unkvoid:name");
        assert_eq!(ROOM_KEY, "unkvoid:last-room");
        assert_eq!(RECENT_KEY, "unkvoid:recent-rooms");
        assert_eq!(TOKEN_KEY, "unkvoid:token");
        assert_eq!(INSTALL_KEY, "unkvoid.instalacao");
    }

    #[test]
    fn the_install_id_is_kept_between_runs() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = App::new(Storage::open_at(dir.path()).expect("open")).install_id();

        assert!(room_code::is_valid(&first));

        let again = App::new(Storage::open_at(dir.path()).expect("reopen")).install_id();

        assert_eq!(first, again, "a instalação mudou de identidade ao reabrir");
    }

    #[test]
    fn the_name_comes_back_on_the_next_run() {
        let dir = tempfile::tempdir().expect("temp dir");

        App::new(Storage::open_at(dir.path()).expect("open"))
            .create_room("Ada", "sala-um")
            .expect("create");

        let again = App::new(Storage::open_at(dir.path()).expect("reopen"));

        assert_eq!(again.state().name, "Ada");
    }
}
