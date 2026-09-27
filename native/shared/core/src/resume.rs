//! Voltar para onde se estava depois que a atualização fechou o app.
//!
//! O instalador mata o processo e abre a versão nova. O que a pessoa fazia — a sala por
//! código ou o canal de voz, e a tela no ar — fica guardado aqui um instante antes, e a versão
//! nova lê uma vez só. Quem assistia volta assistindo sem nada guardado: entrar na sala já
//! abre as telas de quem está transmitindo.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::App;

const RESUME_KEY: &str = "unkvoid:resume";

/// O instalador leva segundos; o aviso do administrador, o tempo que a pessoa quiser. Dez
/// minutos cobrem os dois sem jogar ninguém, horas depois, numa sala que não pediu.
const FRESH: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    /// O código da sala, ou o id do canal de voz.
    pub room: String,
    /// O nome com que se entra numa sala por código.
    pub name: String,
    pub voice: Option<VoiceSeat>,
    /// A tela no ar, como o seletor a manda: `sharing::capture_config` a lê de volta.
    pub share: Option<Value>,
    /// Quando foi guardado, em segundos desde 1970. Quem preenche é o `save`.
    #[serde(default)]
    pub saved_at: u64,
}

/// O canal de voz: o nome que a barra mostra, e o servidor que a tela abre em volta dele.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSeat {
    pub name: String,
    pub server: Option<i64>,
}

pub fn save(app: &App, resume: &Resume) {
    let stamped = Resume {
        saved_at: now(),
        ..resume.clone()
    };

    match serde_json::to_value(stamped) {
        Ok(value) => app.set_preference(RESUME_KEY, value),
        Err(failure) => tracing::warn!(%failure, "retomada: não deu para guardar"),
    }
}

/// Lê e apaga: voltar é uma vez só. Um app que caísse ao voltar cairia de novo a cada abertura.
pub fn take(app: &App) -> Option<Resume> {
    let saved = app.preference(RESUME_KEY)?;

    app.set_preference(RESUME_KEY, Value::Null);

    let resume: Resume = serde_json::from_value(saved).ok()?;

    (now().saturating_sub(resume.saved_at) <= FRESH.as_secs()).then_some(resume)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::Storage;

    fn app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage = Storage::open_at(dir.path()).expect("open");

        (App::new(storage), dir)
    }

    fn sharing_in_voice() -> Resume {
        Resume {
            room: "01j9zchannel".into(),
            name: String::new(),
            voice: Some(VoiceSeat { name: "geral".into(), server: Some(7) }),
            share: Some(serde_json::json!({ "source": "window:42", "quality": "1080", "fps": 60 })),
            saved_at: 0,
        }
    }

    #[test]
    fn what_was_saved_comes_back_once() {
        let (app, _dir) = app();

        save(&app, &sharing_in_voice());

        let back = take(&app).expect("voltou");

        assert_eq!(Resume { saved_at: 0, ..back }, sharing_in_voice());
        assert_eq!(take(&app), None, "voltar é uma vez só");
    }

    #[test]
    fn an_old_resume_is_dropped_instead_of_throwing_someone_into_a_room() {
        let (app, _dir) = app();
        let old = Resume { saved_at: now() - FRESH.as_secs() - 1, ..sharing_in_voice() };

        app.set_preference(RESUME_KEY, serde_json::to_value(old).expect("json"));

        assert_eq!(take(&app), None);
        assert_eq!(app.preference(RESUME_KEY), None, "e sai do disco");
    }
}
