//! O log do app num arquivo por dia, `unkvoid-AAAA-MM-DD.log`, como o canal `daily` do Laravel:
//! quem pede o log de alguém pede o do dia do problema, e os de mais de uma semana somem sozinhos.
//!
//! Antes era um `unkvoid.log` só, recomeçado em 5 MB, e o app antigo (Tauri) deixava outro numa
//! pasta de nome quase igual: pedir "o log" a alguém trazia o arquivo errado.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Quantos dias de log ficam na pasta.
const KEPT_DAYS: usize = 7;

const PREFIX: &str = "unkvoid-";
const SUFFIX: &str = ".log";

// ponytail: sem teto de tamanho por dia. Um aviso em laço pode encher o arquivo do dia; se
// aparecer, cortar a escrita passado um teto (o antigo era 5 MB).
pub struct DailyLog {
    folder: PathBuf,
    day: String,
    file: Option<File>,
}

impl DailyLog {
    /// Abre o arquivo de hoje em `folder` e apaga os logs do formato antigo.
    pub fn open(folder: &Path) -> Self {
        let _ = std::fs::create_dir_all(folder);

        forget_old_logs(folder);

        let mut log = Self { folder: folder.to_owned(), day: String::new(), file: None };

        log.roll(&today());

        log
    }

    fn roll(&mut self, day: &str) {
        day.clone_into(&mut self.day);
        self.file = OpenOptions::new().create(true).append(true).open(self.folder.join(format!("{PREFIX}{day}{SUFFIX}"))).ok();

        let names = std::fs::read_dir(&self.folder)
            .map(|entries| entries.filter_map(|entry| entry.ok()?.file_name().into_string().ok()).collect())
            .unwrap_or_default();

        for name in stale(names) {
            let _ = std::fs::remove_file(self.folder.join(name));
        }
    }
}

impl Write for DailyLog {
    /// O dia é conferido a cada linha: o app fica aberto de um dia para o outro, na bandeja.
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let day = today();

        if day != self.day {
            self.roll(&day);
        }

        match &mut self.file {
            Some(file) => file.write(buffer),
            None => Ok(buffer.len()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.as_mut().map_or(Ok(()), File::flush)
    }
}

/// O dia de hoje no relógio da máquina, `AAAA-MM-DD`: o nome do arquivo bate com a data que a
/// pessoa vê no canto da tela.
fn today() -> String {
    let now = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };

    format!("{:04}-{:02}-{:02}", now.wYear, now.wMonth, now.wDay)
}

/// Os arquivos diários que passaram dos `KEPT_DAYS` mais novos. O nome é a data, então a
/// ordem do nome é a ordem do tempo.
fn stale(names: Vec<String>) -> Vec<String> {
    let mut daily: Vec<String> = names
        .into_iter()
        .filter(|name| name.strip_prefix(PREFIX).and_then(|rest| rest.strip_suffix(SUFFIX)).is_some_and(|day| day.len() == 10))
        .collect();

    daily.sort_unstable_by(|left, right| right.cmp(left));
    daily.split_off(KEPT_DAYS.min(daily.len()))
}

/// O log de antes dos diários, ao lado, e o do app antigo em `%LOCALAPPDATA%\unkvoid`. A pasta
/// do antigo só sai se ficar vazia: nada além dos dois arquivos dele é apagado.
fn forget_old_logs(folder: &Path) {
    for name in ["unkvoid.log", "unkvoid.old.log"] {
        let _ = std::fs::remove_file(folder.join(name));
    }

    let tauri = folder.with_file_name("unkvoid");

    for name in ["unkvoid.log", "unkvoid.sent"] {
        let _ = std::fs::remove_file(tauri.join(name));
    }

    let _ = std::fs::remove_dir(tauri);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_daily_files_past_the_newest_week_go() {
        let mut names: Vec<String> = (1..=9).map(|day| format!("unkvoid-2026-10-{day:02}.log")).collect();

        names.extend(["unkvoid.log".into(), "clips.json".into(), "unkvoid-x.log".into()]);

        let mut gone = stale(names);

        gone.sort();

        assert_eq!(gone, ["unkvoid-2026-10-01.log", "unkvoid-2026-10-02.log"]);
    }

    #[test]
    fn a_week_or_less_keeps_everything() {
        assert!(stale(vec!["unkvoid-2026-10-01.log".into()]).is_empty());
    }

    #[test]
    fn today_is_a_ten_character_date() {
        let day = today();

        assert_eq!(day.len(), 10, "{day}");
        assert_eq!(&day[4..5], "-");
    }
}
