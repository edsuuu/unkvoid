//! O estado que as três interfaces desenham, e as decisões que nenhuma delas toma sozinha.
//!
//! Entrar numa sala parece coisa de tela — mas envolve validar o código, guardar o nome, a
//! sala e a lista das recentes. Escrito três vezes, seriam três regras que divergem no
//! primeiro ajuste. Aqui é uma.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

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

/// As chaves do React (`Sharing.ts`): quem trocar de app leva a escolha junto.
const QUALITY_KEY: &str = "unkvoid:quality";
const FPS_KEY: &str = "unkvoid:fps";

/// O que o seletor de tela oferece, na ordem do React.
pub const QUALITIES: [&str; 4] = ["720", "1080", "1440", "2160"];
pub const FRAME_RATES: [&str; 2] = ["30", "60"];

const TOKEN_KEY: &str = "unkvoid:token";
const REFRESH_KEY: &str = "unkvoid:refresh";

const INSTALL_KEY: &str = "unkvoid.instalacao";

/// Quantas salas recentes a Home mostra. Três, por decisão do dono: passou disso vira uma
/// lista que ninguém lê.
const MAX_RECENT: usize = 3;

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
        let code = if typed.is_empty() {
            room_code::generate()
        } else {
            typed
        };

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
            .take(MAX_RECENT)
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

        if let Err(failure) = self
            .storage
            .set(INSTALL_KEY, serde_json::Value::String(fresh.clone()))
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

    /// Uma preferência guardada (microfone, qualidade, teclas…). As chaves são as que o app
    /// de hoje já grava (`unkvoid:voice`, `unkvoid:quality`…), e o token **não** sai por
    /// aqui: ele é cifrado e tem o caminho dele.
    /// Preferência qualquer, menos os dois tokens: eles só entram e saem cifrados, pelo
    /// `token` e pelo `keep_session`.
    pub fn preference(&self, key: &str) -> Option<serde_json::Value> {
        (key != TOKEN_KEY && key != REFRESH_KEY).then(|| self.storage.get(key)).flatten()
    }

    pub fn set_preference(&self, key: &str, value: serde_json::Value) {
        if key == TOKEN_KEY || key == REFRESH_KEY {
            return;
        }

        let saved = if value.is_null() {
            self.storage.remove(key)
        } else {
            self.storage.set(key, value)
        };

        if let Err(failure) = saved {
            tracing::warn!(%failure, key, "a preferência não foi guardada");
        }
    }

    /// O token do Sanctum, decifrado. Sem chaveiro não há token guardado, e o app pede
    /// login de novo — que é melhor do que deixá-lo legível no disco.
    ///
    /// Token guardado que não abre — cifrado por outro build, com outra chave, ou num chaveiro
    /// que sumiu — não é sessão: sai do disco na hora. Sem isto a tela contava a conta pela
    /// chave no arquivo, abria o hub sem conta, e a pessoa ficava presa lá sem voltar ao login.
    pub fn token(&self) -> Option<String> {
        let token = self.cipher().and_then(|cipher| self.storage.get_secret(TOKEN_KEY, cipher));

        if token.is_none() && self.has_token() {
            tracing::info!("o token guardado não abre: a sessão volta para a entrada");
            self.set_token(None);
        }

        token
    }

    pub fn refresh_token(&self) -> Option<String> {
        self.storage.get_secret(REFRESH_KEY, self.cipher()?)
    }

    fn set_refresh_token(&self, refresh: &str) {
        let Some(cipher) = self.cipher() else {
            return;
        };

        if let Err(failure) = self.storage.set_secret(REFRESH_KEY, refresh, cipher) {
            tracing::warn!(%failure, "o token de renovação não foi guardado");
        }
    }

    /// Liga a sessão da `Api` ao disco: o par renovado vai para o chaveiro, e a sessão que
    /// acabou sai dele — e aí `ended` leva a interface de volta ao login. Chamado uma vez, na
    /// abertura, antes de qualquer pedido com conta.
    pub fn keep_session(self: &Arc<Self>, api: &crate::api::Api, ended: impl Fn() + Send + Sync + 'static) {
        api.set_refresh(self.refresh_token());

        let app = Arc::clone(self);

        api.on_session(move |renewal| match renewal {
            crate::api::Renewal::Renewed { token, refresh } => {
                app.set_token(Some(token));
                app.set_refresh_token(refresh);
            }
            crate::api::Renewal::Ended => {
                app.set_token(None);
                ended();
            }
        });
    }

    /// Sem token não há par: apagar um apaga o outro.
    pub fn set_token(&self, token: Option<&str>) {
        self.signed_in.store(token.is_some(), Ordering::Relaxed);

        let Some(token) = token else {
            for key in [TOKEN_KEY, REFRESH_KEY] {
                if let Err(failure) = self.storage.remove(key) {
                    tracing::warn!(%failure, key, "não deu para apagar o token");
                }
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

    /// A qualidade e o fps com que o seletor de tela abre: a última escolha, ou o palpite do
    /// React pelo número de núcleos.
    pub fn share_quality(&self) -> (String, String) {
        let cores = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        let (quality, fps) = guessed_quality(cores, cfg!(target_os = "linux"));
        let saved = |key: &str, allowed: &[&str], guess: &str| {
            self.storage
                .get_string(key)
                .filter(|value| allowed.contains(&value.as_str()))
                .unwrap_or_else(|| guess.to_owned())
        };

        (saved(QUALITY_KEY, &QUALITIES, quality), saved(FPS_KEY, &FRAME_RATES, fps))
    }

    pub fn set_share_quality(&self, quality: &str, fps: &str) {
        self.set_preference(QUALITY_KEY, serde_json::json!(quality));
        self.set_preference(FPS_KEY, serde_json::json!(fps));
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
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// O palpite do React: máquina com mais de oito núcleos aguenta 1080p60; com até quatro, 720p30;
/// no meio, 1080p60 — menos no Linux, onde o encoder costuma ser o do processador e fica em 30.
fn guessed_quality(cores: usize, linux: bool) -> (&'static str, &'static str) {
    if cores > 8 {
        return ("1080", "60");
    }

    if cores <= 4 {
        return ("720", "30");
    }

    ("1080", if linux { "30" } else { "60" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_share_quality_guess_is_the_react_one_and_a_choice_sticks() {
        assert_eq!(guessed_quality(16, false), ("1080", "60"));
        assert_eq!(guessed_quality(4, false), ("720", "30"));
        assert_eq!(guessed_quality(6, true), ("1080", "30"));
        assert_eq!(guessed_quality(6, false), ("1080", "60"));

        let (app, _dir) = app();

        app.set_share_quality("1440", "30");

        assert_eq!(app.share_quality(), ("1440".to_owned(), "30".to_owned()));
    }

    #[test]
    fn the_tokens_never_leave_through_the_preferences() {
        let (app, _dir) = app();

        app.set_preference(REFRESH_KEY, serde_json::json!("trocado"));

        assert_eq!(app.preference(REFRESH_KEY), None);
        assert_eq!(app.preference(TOKEN_KEY), None);
    }

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage = Storage::open_at(dir.path()).expect("open");

        (App::new(storage), dir)
    }

    #[test]
    fn a_stored_token_that_does_not_open_lands_on_the_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage = Storage::open_at(dir.path()).expect("open");

        storage.set(TOKEN_KEY, serde_json::json!("cifrado-por-outra-chave")).expect("write");

        let app = App::new(storage);

        assert_eq!(app.token(), None);
        assert!(!app.has_token(), "a chave no disco não é sessão");
        assert_eq!(app.home(), Screen::Entry);
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

        assert_eq!(
            app.join_room("Ada", "  SALA-DO-EDSU  ").expect("join"),
            "sala-do-edsu"
        );
    }

    #[test]
    fn entering_without_a_name_is_refused() {
        let (app, _dir) = app();

        assert_eq!(
            app.create_room("   ", "").unwrap_err(),
            EntryRefusal::NameIsEmpty
        );
        assert_eq!(
            app.state().screen,
            Screen::Entry,
            "mudou de tela mesmo recusando"
        );
    }

    #[test]
    fn a_malformed_code_is_refused() {
        let (app, _dir) = app();

        assert_eq!(
            app.join_room("Ada", "-x-").unwrap_err(),
            EntryRefusal::CodeIsInvalid
        );
        assert!(app.state().room.is_none());
    }

    /// Quem já tinha uma lista maior, gravada quando o teto era outro, também vê só três.
    #[test]
    fn only_the_last_three_rooms_are_listed() {
        let (app, _dir) = app();
        let saved = serde_json::json!([
            "sala-um",
            "sala-dois",
            "sala-tres",
            "sala-quatro",
            "sala-cinco"
        ]);

        app.storage.set(RECENT_KEY, saved).expect("write");

        assert_eq!(app.recent_rooms(), ["sala-um", "sala-dois", "sala-tres"]);

        app.join_room("Ada", "sala-nova").expect("join");

        assert_eq!(
            app.recent_rooms(),
            ["sala-nova", "sala-um", "sala-dois"],
            "a mais nova entra e a mais antiga sai"
        );
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
            app.join_room("Ada", &format!("sala-{index}"))
                .expect("join");
        }

        assert_eq!(app.recent_rooms().len(), MAX_RECENT);
    }

    #[test]
    fn an_invalid_code_saved_by_an_older_version_is_ignored() {
        let (app, _dir) = app();

        app.storage
            .set(RECENT_KEY, serde_json::json!(["boa", "-ruim-", 42]))
            .expect("write");

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

        assert_eq!(
            app.state().screen,
            Screen::Hub,
            "quem tem conta volta para os servidores"
        );
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

        assert_eq!(
            first, again,
            "trocar de id derrubaria a própria sessão anterior na sala"
        );
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
