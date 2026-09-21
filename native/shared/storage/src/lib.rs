//! O estado do app em disco, na pasta que cada sistema reserva para ele.
//!
//! Substitui o `localStorage` da janela web, que guarda dentro do perfil da webview: some
//! ao limpar dados do navegador, não é o mesmo em cada sistema e não dá para abrir num
//! editor quando alguém precisa contar o que está gravado.
//!
//! Um arquivo JSON só, gravado por inteiro a cada escrita. São dez chaves pequenas, não um
//! banco — e um arquivo que se lê de olho vale mais aqui do que um formato esperto.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

mod secret;

pub use secret::Cipher;

/// O mesmo identificador do bundle, para cair na pasta que o instalador já usa.
const APP_DIR: &str = "com.unkvoid.desktop";

const FILE: &str = "state.json";

/// O nome do campo que marca um valor cifrado. Quem abrir o arquivo vê que ali há algo
/// guardado, e não um texto que alguém esqueceu de ler.
const SEALED: &str = "enc";

pub struct Storage {
    path: PathBuf,
    /// O conteúdo vive em memória: leitura é o caso comum e não merece um acesso a disco.
    state: Mutex<Map<String, Value>>,
}

impl Storage {
    /// A pasta do sistema: `Application Support` no macOS, `AppData` no Windows,
    /// `~/.config` no Linux.
    ///
    /// `UNKVOID_STATE_DIR` manda em tudo. Serve para duas coisas que não são luxo: teste que
    /// não pode depender do que está gravado na máquina de quem roda, e **duas instâncias do
    /// app lado a lado** — que é como se prova uma sala com duas pessoas sem precisar de um
    /// segundo computador.
    pub fn open() -> Result<Self> {
        if let Some(dir) = std::env::var_os("UNKVOID_STATE_DIR") {
            return Self::open_at(Path::new(&dir));
        }

        let dir = dirs::config_dir()
            .context("o sistema não informou a pasta de configuração")?
            .join(APP_DIR);

        Self::open_at(&dir)
    }

    pub fn open_at(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir)
            .with_context(|| format!("não deu para criar {}", dir.display()))?;

        let path = dir.join(FILE);
        let state = Self::read(&path)?;

        Ok(Self { path, state: Mutex::new(state) })
    }

    fn read(path: &Path) -> Result<Map<String, Value>> {
        let Ok(raw) = fs::read_to_string(path) else {
            return Ok(Map::new());
        };

        match serde_json::from_str::<Map<String, Value>>(&raw) {
            Ok(state) => Ok(state),
            // Arquivo corrompido não pode impedir o app de abrir: o que está aqui é
            // preferência e token, e perder isso custa um login, não os dados de ninguém.
            Err(error) => {
                tracing::warn!(%error, "estado ilegível no disco, recomeçando vazio");

                Ok(Map::new())
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.lock().get(key).cloned()
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Value::String(text) => Some(text),
            other => Some(other.to_string()),
        }
    }

    pub fn set(&self, key: &str, value: Value) -> Result<()> {
        self.lock().insert(key.to_owned(), value);

        self.flush()
    }

    pub fn remove(&self, key: &str) -> Result<()> {
        self.lock().remove(key);

        self.flush()
    }

    pub fn keys(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    /// Traz o que a janela web gravava, sem passar por cima do que já existe aqui: quem já
    /// migrou não volta para o valor antigo se abrir uma versão velha do app no meio.
    pub fn import_missing(&self, from: &Map<String, Value>) -> Result<usize> {
        let mut state = self.lock();
        let mut added = 0;

        for (key, value) in from {
            if !state.contains_key(key) {
                state.insert(key.clone(), value.clone());
                added += 1;
            }
        }

        drop(state);

        if added > 0 {
            self.flush()?;
        }

        Ok(added)
    }

    /// Grava cifrado. A chave vive no chaveiro do sistema, nunca neste arquivo.
    pub fn set_secret(&self, key: &str, value: &str, cipher: &Cipher) -> Result<()> {
        self.set(key, json!({ SEALED: cipher.encrypt(value)? }))
    }

    /// Devolve em claro, para uso em memória.
    ///
    /// Valor que não abre volta como `None` e não como erro: chaveiro trocado ou
    /// arquivo copiado de outra máquina valem um login novo, não uma tela de falha.
    pub fn get_secret(&self, key: &str, cipher: &Cipher) -> Option<String> {
        let sealed = self.get(key)?;
        let sealed = sealed.get(SEALED)?.as_str()?;

        match cipher.decrypt(sealed) {
            Ok(plain) => Some(plain),
            Err(error) => {
                tracing::warn!(%error, key, "valor guardado não abriu");

                None
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A pasta onde o estado mora. O que é guardado ao lado dele — como a chave de
    /// desenvolvimento — some junto quando alguém limpa tudo.
    pub fn directory(&self) -> &Path {
        self.path.parent().unwrap_or(&self.path)
    }

    /// Grava num arquivo ao lado e troca por cima. Escrita direta que morre no meio —
    /// bateria acabando, sistema desligando — deixaria um JSON pela metade, e o app abriria
    /// sem token e sem preferência nenhuma.
    fn flush(&self) -> Result<()> {
        // O cadeado fica até a troca: duas gravações ao mesmo tempo dividem o mesmo arquivo
        // ao lado, e uma trocaria por cima o que a outra ainda estava escrevendo.
        let state = self.lock();
        let body = serde_json::to_string_pretty(&*state)?;
        let temporary = self.path.with_extension("json.tmp");

        fs::write(&temporary, body)
            .with_context(|| format!("não deu para gravar {}", temporary.display()))?;

        fs::rename(&temporary, &self.path)
            .with_context(|| format!("não deu para trocar {}", self.path.display()))?;

        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Map<String, Value>> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    fn storage() -> (Storage, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage = Storage::open_at(dir.path()).expect("open");

        (storage, dir)
    }

    /// O modal de compartilhar grava a qualidade e os quadros por segundo quase juntos, de
    /// threads diferentes: nenhuma das duas gravações pode falhar nem sumir.
    #[test]
    fn writing_from_many_threads_at_once_loses_nothing() {
        let (storage, dir) = storage();

        std::thread::scope(|scope| {
            for index in 0..16 {
                let storage = &storage;

                scope.spawn(move || storage.set(&format!("key-{index}"), json!(index)).expect("set"));
            }
        });

        let reopened = Storage::open_at(dir.path()).expect("reopen");

        for index in 0..16 {
            assert_eq!(reopened.get(&format!("key-{index}")), Some(json!(index)));
        }
    }

    #[test]
    fn writes_and_reads_back_from_disk() {
        let (storage, dir) = storage();

        storage.set("unkvoid:token", json!("abc123")).expect("write");

        assert_eq!(storage.get_string("unkvoid:token").as_deref(), Some("abc123"));

        let reopened = Storage::open_at(dir.path()).expect("reopen");

        assert_eq!(reopened.get_string("unkvoid:token").as_deref(), Some("abc123"));
    }

    #[test]
    fn remove_deletes_from_disk() {
        let (storage, dir) = storage();

        storage.set("name", json!("Ada")).expect("write");
        storage.remove("name").expect("remove");

        assert!(Storage::open_at(dir.path()).expect("reopen").get("name").is_none());
    }

    #[test]
    fn a_corrupt_file_does_not_stop_the_app_from_opening() {
        let dir = tempfile::tempdir().expect("temp dir");

        fs::write(dir.path().join(FILE), "{this is not json").expect("corrupt it");

        let storage = Storage::open_at(dir.path()).expect("open even when dirty");

        assert!(storage.keys().is_empty());

        storage.set("name", json!("Ada")).expect("overwrite");

        assert_eq!(storage.get_string("name").as_deref(), Some("Ada"));
    }

    #[test]
    fn importing_never_overwrites_what_is_already_here() {
        let (storage, _dir) = storage();

        storage.set("name", json!("new")).expect("write");

        let mut from_web = Map::new();

        from_web.insert("name".into(), json!("old"));
        from_web.insert("room".into(), json!("abc-def"));

        assert_eq!(storage.import_missing(&from_web).expect("import"), 1);
        assert_eq!(storage.get_string("name").as_deref(), Some("new"));
        assert_eq!(storage.get_string("room").as_deref(), Some("abc-def"));
    }

    #[test]
    fn the_token_is_not_readable_in_the_file() {
        let (storage, dir) = storage();
        let cipher = Cipher::test_only();

        storage.set_secret("unkvoid:token", "1|sanctum-plain-text-token", &cipher).expect("write");

        let raw = fs::read_to_string(dir.path().join(FILE)).expect("read the file");

        assert!(!raw.contains("sanctum-plain-text-token"), "the token is readable on disk:\n{raw}");
        assert!(raw.contains(SEALED));

        // Em memória volta em claro, que é como o app usa.
        assert_eq!(
            storage.get_secret("unkvoid:token", &cipher).as_deref(),
            Some("1|sanctum-plain-text-token"),
        );
    }

    #[test]
    fn a_token_from_another_machine_fails_without_breaking() {
        let (storage, _dir) = storage();

        storage
            .set_secret("unkvoid:token", "1|sanctum-plain-text-token", &Cipher::test_only())
            .expect("write");

        assert!(storage.get_secret("unkvoid:token", &Cipher::test_only()).is_none());
    }

    #[test]
    fn leaves_no_temporary_file_behind() {
        let (storage, dir) = storage();

        storage.set("name", json!("Ada")).expect("write");

        let left: Vec<_> = fs::read_dir(dir.path())
            .expect("list")
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "tmp"))
            .collect();

        assert!(left.is_empty(), "a temporary file was left behind: {left:?}");
    }
}
