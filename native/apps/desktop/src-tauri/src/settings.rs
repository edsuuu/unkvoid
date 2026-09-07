//! Configurações que ficam na máquina, não na conta.
//!
//! Atalho de teclado, monitor escolhido para transmitir, iniciar com o sistema: nada
//! disso faz sentido viajar entre computadores. O servidor guarda quem é a pessoa e o
//! que ela pode; isto aqui guarda como *esta* máquina se comporta.
//!
//! SQLite e não um JSON solto porque o app escreve daqui e do webview ao mesmo tempo —
//! um arquivo reescrito inteiro a cada tecla perde dados quando duas escritas se cruzam,
//! e um desligamento no meio deixa o arquivo truncado. O SQLite resolve os dois.

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

pub struct Settings(Mutex<Connection>);

impl Settings {
    /// Abre (ou cria) o banco no diretório de dados do app.
    pub fn open(path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;

        // WAL: o webview lê enquanto o Rust escreve, sem um bloquear o outro.
        connection.pragma_update(None, "journal_mode", "WAL")?;

        connection.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            (),
        )?;

        Ok(Self(Mutex::new(connection)))
    }

    pub fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let connection = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;

        let mut statement = connection.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = statement.query([key])?;

        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    pub fn set(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let connection = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;

        connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )?;

        Ok(())
    }

    pub fn all(&self) -> anyhow::Result<Vec<(String, String)>> {
        let connection = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;

        let mut statement = connection.prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;

        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um arquivo por teste: os testes rodam em paralelo na mesma thread pool, e
    /// compartilhar o banco faz um enxergar o que o outro escreveu.
    fn temporario(nome: &str) -> Settings {
        let caminho =
            std::env::temp_dir().join(format!("unkvoid-{nome}-{}.db", std::process::id()));

        let _ = std::fs::remove_file(&caminho);

        Settings::open(caminho).expect("could not open the settings database")
    }

    #[test]
    fn guarda_e_devolve() {
        let settings = temporario("get");

        assert_eq!(settings.get("push_to_talk").unwrap(), None);

        settings.set("push_to_talk", "ControlLeft").unwrap();

        assert_eq!(
            settings.get("push_to_talk").unwrap().as_deref(),
            Some("ControlLeft")
        );
    }

    #[test]
    fn gravar_de_novo_substitui_em_vez_de_duplicar() {
        let settings = temporario("upsert");

        settings.set("quality", "1080").unwrap();
        settings.set("quality", "1440").unwrap();

        assert_eq!(settings.get("quality").unwrap().as_deref(), Some("1440"));
        assert_eq!(settings.all().unwrap().len(), 1);
    }

    #[test]
    fn sobrevive_a_reabrir_o_banco() {
        let caminho =
            std::env::temp_dir().join(format!("unkvoid-persist-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&caminho);

        Settings::open(caminho.clone())
            .unwrap()
            .set("monitor", "1")
            .unwrap();

        // O ponto do banco é justamente este: fechar o app não pode perder o bind.
        assert_eq!(
            Settings::open(caminho)
                .unwrap()
                .get("monitor")
                .unwrap()
                .as_deref(),
            Some("1")
        );
    }
}
