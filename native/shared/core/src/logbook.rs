//! O log do app num arquivo por dia, `unkvoid-AAAA-MM-DD.log`, como o canal `daily` do Laravel:
//! quem pede o log de alguém pede o do dia do problema, e os de mais de uma semana somem sozinhos.
//! E o pedaço do dia que ganhou um `ERROR` vai ao site (`POST /api/errors`), sem o nome de quem
//! usa a máquina: é assim que o congelamento de alguém chega a quem conserta.
//!
//! Mora no núcleo porque é o mesmo nos três sistemas; cada um só diz em que pasta.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::api::Api;

/// Quantos dias de log ficam na pasta.
const KEPT_DAYS: usize = 7;

const PREFIX: &str = "unkvoid-";
const SUFFIX: &str = ".log";

/// O que vai ao site num relatório: o servidor recusa mais de 20 mil caracteres.
const MOST_REPORTED: usize = 19_000;

/// O quanto do log de hoje já foi relatado, `AAAA-MM-DD bytes`, ao lado dos logs.
const MARKER: &str = "unkvoid.sent";

/// De quanto em quanto tempo o log do dia é conferido.
const REPORT_EVERY: Duration = Duration::from_secs(30);

// ponytail: sem teto de tamanho por dia. Um aviso em laço pode encher o arquivo do dia; se
// aparecer, cortar a escrita passado um teto (o antigo do Windows era 5 MB).
pub struct DailyLog {
    folder: PathBuf,
    day: String,
    file: Option<File>,
}

impl DailyLog {
    /// Abre o arquivo de hoje em `folder`, criando a pasta.
    pub fn open(folder: &Path) -> Self {
        let _ = std::fs::create_dir_all(folder);

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
    /// O dia é conferido a cada linha: o app fica aberto de um dia para o outro.
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

/// O pedaço do log de hoje que ainda não foi ao site.
pub struct Unreported {
    /// O fim do pedaço, até o tamanho que o site aceita, sem o nome de quem usa a máquina.
    pub log: String,
    marker: PathBuf,
    mark: String,
}

impl Unreported {
    /// O site guardou: o marcador anda. Sem isto o mesmo pedaço volta no próximo relatório,
    /// que é o que se quer quando o envio falhou.
    pub fn sent(&self) {
        let _ = std::fs::write(&self.marker, &self.mark);
    }
}

/// O que o log de hoje ganhou desde o último relatório, quando há erro nele (`ERROR`, onde
/// cai também o pânico). Pedaço sem erro só faz o marcador andar: não precisa ser lido de novo.
pub fn unreported(folder: &Path) -> Option<Unreported> {
    let day = today();
    let mut file = File::open(folder.join(format!("{PREFIX}{day}{SUFFIX}"))).ok()?;
    let length = file.metadata().ok()?.len();
    let marker = folder.join(MARKER);
    let from = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|text| {
            let (marked, offset) = text.trim().split_once(' ')?;

            (marked == day).then(|| offset.parse::<u64>().ok()).flatten()
        })
        .filter(|&offset| offset <= length)
        .unwrap_or(0);
    let mark = format!("{day} {length}");
    let mut fresh = Vec::new();

    // Só o que entrou depois do último relatório: o log do dia pode ter megabytes.
    file.seek(SeekFrom::Start(from)).ok()?;
    (&mut file).take(length - from).read_to_end(&mut fresh).ok()?;

    let slice = String::from_utf8_lossy(&fresh);

    if !slice.contains(" ERROR ") {
        let _ = std::fs::write(&marker, mark);

        return None;
    }

    let characters: Vec<char> = slice.chars().collect();
    let tail: String = characters[characters.len().saturating_sub(MOST_REPORTED)..].iter().collect();
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();

    Some(Unreported { log: scrub(&tail, &user), marker, mark })
}

/// Manda ao site, de `REPORT_EVERY` em `REPORT_EVERY`, o pedaço do log do dia que ganhou um
/// erro. Roda enquanto o app estiver aberto.
pub async fn report_errors(api: &Api, folder: &Path) {
    loop {
        if let Some(pending) = unreported(folder)
            && api.report_error(env!("CARGO_PKG_VERSION"), std::env::consts::OS, &pending.log).await
        {
            pending.sent();
        }

        tokio::time::sleep(REPORT_EVERY).await;
    }
}

/// Tira o nome de quem usa a máquina: todo caminho passa por `C:\Users\<nome>` ou
/// `/home/<nome>`, e o relatório precisa de onde o arquivo estava, não de quem estava na frente
/// do computador.
fn scrub(text: &str, user: &str) -> String {
    // Nome de duas letras aparece dentro de palavra, e trocá-lo estragaria o resto do log.
    if user.chars().count() < 3 {
        return text.to_owned();
    }

    text.replace(user, "<usuario>")
}

/// O dia de hoje no relógio da máquina, `AAAA-MM-DD`: o nome do arquivo bate com a data que a
/// pessoa vê no canto da tela.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
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
        assert!(stale(vec!["unkvoid-2026-10-01.log".into()]).is_empty(), "uma semana ou menos fica inteira");
    }

    #[test]
    fn only_a_slice_with_an_error_is_reported_and_only_once() {
        let folder = std::env::temp_dir().join(format!("unkvoid-logbook-core-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(format!("{PREFIX}{}{SUFFIX}", today()));

        std::fs::write(&file, "2026-10-02T10:00:00Z  INFO core_app: tudo certo\n").unwrap();
        assert!(unreported(&folder).is_none(), "pedaço sem erro não vai ao site");

        let mut appending = OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(appending, "2026-10-02T10:00:01Z ERROR core_app::room: assistir: a imagem ficou parada").unwrap();

        let pending = unreported(&folder).expect("o erro vai ao site");

        assert!(pending.log.contains("a imagem ficou parada"));
        assert!(!pending.log.contains("tudo certo"), "o pedaço já marcado não volta");
        assert!(unreported(&folder).is_some(), "sem o site confirmar, o pedaço continua pendente");

        pending.sent();
        assert!(unreported(&folder).is_none(), "depois de enviado, não volta");

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_user_name_leaves_the_report() {
        assert_eq!(scrub(r"C:\Users\mank\AppData", "mank"), r"C:\Users\<usuario>\AppData");
        assert_eq!(scrub("/home/mank/.local/state", "mank"), "/home/<usuario>/.local/state");
        assert_eq!(scrub("ed foi", "ed"), "ed foi", "nome curto demais fica");
    }

    #[test]
    fn today_is_a_ten_character_date() {
        let day = today();

        assert_eq!(day.len(), 10, "{day}");
        assert_eq!(&day[4..5], "-");
    }
}
