//! O caderno de bordo do app: tudo em arquivo, sempre.
//!
//! No Windows o executável é de subsistema gráfico — não existe console, e cada linha
//! que o `tracing` mandava para a saída padrão era jogada fora. Quando clicar em
//! transmitir derrubava o processo, não sobrava um caractere para dizer onde. Aqui cada
//! linha vai para disco na hora, então a **última linha antes do silêncio** é a pista:
//! vale para pânico do Rust, para erro devolvido à interface e para a morte suja que
//! nenhum hook pega, como violação de acesso dentro de uma chamada COM.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Acima disto o arquivo recomeça. Log que enche o disco de quem só queria assistir a
/// uma tela é pior do que log nenhum.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Teto de uma linha vinda da interface. Ela é nossa, mas é a única entrada de fora
/// deste módulo, e um `JSON.stringify` de algo inesperado não pode virar um arquivo de
/// gigabytes.
const MAX_LINE: usize = 4_000;

/// Quanto do log viaja num relatório. O servidor recusa acima de 20 000 caracteres.
const MAX_REPORT: u64 = 18_000;

/// Para onde vai o relatório de erro. Fixo, como o endereço do atualizador: é o mesmo
/// servidor, e deixar isso configurável só daria a quem mexesse na configuração um jeito
/// de mandar o log de outra pessoa para outro lugar.
const REPORT_URL: &str = "https://unkvoid.com/api/errors";

static FILE: OnceLock<Mutex<File>> = OnceLock::new();

/// Onde o arquivo mora em cada sistema, no lugar que cada um chama de seu.
///
/// Sem dependência nova: são três variáveis de ambiente que os três sistemas garantem.
/// Faltando todas, o diretório temporário — um log que some no reinício ainda é melhor
/// do que nenhum.
pub fn path() -> PathBuf {
    let (base, folder) = if cfg!(target_os = "windows") {
        (std::env::var_os("LOCALAPPDATA"), "unkvoid")
    } else if cfg!(target_os = "macos") {
        (std::env::var_os("HOME"), "Library/Logs/unkvoid")
    } else {
        (std::env::var_os("HOME"), ".local/state/unkvoid")
    };

    base.map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(folder)
        .join("unkvoid.log")
}

/// O arquivo ao lado do log com quantos bytes dele já foram relatados.
fn marker_path() -> PathBuf {
    path().with_extension("sent")
}

/// Abre o arquivo, aponta o `tracing` para ele e instala o hook de pânico.
///
/// Chamado no começo do `run`, antes de qualquer coisa que possa morrer. A versão vem de
/// fora porque a do `CARGO_PKG_VERSION` é a do workspace, e não a que a pessoa instalou:
/// o cabeçalho dizia `unkvoid 0.1.0` em toda máquina, em toda versão.
pub fn init(version: &str) {
    let path = path();

    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }

    if std::fs::metadata(&path).is_ok_and(|data| data.len() > MAX_BYTES) {
        let _ = std::fs::remove_file(&path);
    }

    // Dois handles do mesmo arquivo em modo append: um para o `tracing`, outro para as
    // linhas cruas. O append é do sistema, então os dois nunca escrevem por cima.
    let handles = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|file| file.try_clone().map(|copy| (file, copy)));

    let Ok((file, for_tracing)) = handles else {
        // Sem arquivo o app continua: no macOS e no Linux o terminal ainda mostra.
        tracing_subscriber::fmt().with_env_filter("info").init();

        return;
    };

    let _ = FILE.set(Mutex::new(file));

    // Um pânico na thread da captura morria calado — a thread ia embora e o app seguia
    // sem imagem e sem erro. O hook precisa do `FILE` já pronto, por isso vem depois.
    std::panic::set_hook(Box::new(|info| {
        let onde = info
            .location()
            .map_or_else(|| "?".to_string(), |local| format!("{local}"));

        write(&format!("panic {onde} :: {info}"));
        write(&format!(
            "backtrace ::\n{}",
            std::backtrace::Backtrace::force_capture()
        ));
    }));

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_ansi(false)
        .with_writer(Mutex::new(for_tracing))
        .init();

    write(&format!(
        "--- unkvoid {} em {} ---",
        version,
        std::env::consts::OS
    ));
}

/// O pedaço do log que ainda não foi relatado, e a posição nova do marcador.
///
/// Fica com o **fim** do pedaço quando ele passa do teto: numa sessão longa o pânico é a
/// última coisa que aconteceu, e cortar pelo começo mandaria justamente a parte em que
/// estava tudo bem.
///
/// Lê bytes e converte com perda porque o corte pode cair no meio de um caractere. Um
/// acento torto no relatório é melhor do que relatório nenhum.
fn unreported(log: &Path, marker: &Path) -> Option<(String, u64)> {
    use std::io::{Read, Seek, SeekFrom};

    let size = std::fs::metadata(log).ok()?.len();
    let sent = std::fs::read_to_string(marker)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .unwrap_or(0);

    // Marcador maior que o arquivo quer dizer que o log recomeçou por causa do teto de
    // tamanho: o que está lá agora é todo novo.
    let from = if sent > size { 0 } else { sent };

    if from >= size {
        return None;
    }

    let mut file = File::open(log).ok()?;
    let mut bytes = Vec::new();

    file.seek(SeekFrom::Start(from.max(size.saturating_sub(MAX_REPORT))))
        .ok()?;
    file.take(MAX_REPORT).read_to_end(&mut bytes).ok()?;

    Some((String::from_utf8_lossy(&bytes).into_owned(), size))
}

fn mark_reported(marker: &Path, offset: u64) {
    let _ = std::fs::write(marker, offset.to_string());
}

/// Tira o nome de usuário da máquina do que vai ser enviado.
///
/// Todo caminho de arquivo no Windows passa por `C:\Users\<nome>`, e o log é feito de
/// caminho. Quem lê o relatório precisa saber onde o arquivo estava, não quem estava na
/// frente do computador.
fn scrub(text: &str, user: &str) -> String {
    // Nome de duas letras aparece dentro de palavra, e trocá-lo estragaria o resto do
    // log — que é a única coisa que o relatório tem.
    if user.chars().count() < 3 {
        return text.to_string();
    }

    text.replace(user, "<usuario>")
}

/// Como o sistema chama quem está usando a máquina. É o que sai do log antes de viajar.
fn current_user() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

/// Manda para o servidor o que aconteceu desde o último relatório, quando houve erro.
///
/// Na abertura, porque é o único momento possível: um pânico ou uma morte suja dentro de
/// uma chamada do sistema leva o processo junto, e não sobra ninguém para avisar. Na vez
/// seguinte o pedaço do log ainda está no disco, e é ele que viaja.
///
/// Falhar aqui não muda nada para quem está abrindo o app: o marcador só anda quando o
/// servidor confirma, então o relatório tenta de novo na próxima abertura.
pub fn report(version: &str) {
    let marker = marker_path();

    let Some((slice, offset)) = unreported(&path(), &marker) else {
        return;
    };

    // Sem erro dentro não há o que mapear. O pedaço fica marcado como visto para não ser
    // lido de novo na próxima abertura.
    if ! slice.contains("panic ") && ! slice.contains("ERROR") {
        mark_reported(&marker, offset);

        return;
    }

    let body = serde_json::json!({
        "version": version,
        "platform": std::env::consts::OS,
        "log": scrub(&slice, &current_user()),
    });

    tauri::async_runtime::spawn(async move {
        let sent = reqwest::Client::new()
            .post(REPORT_URL)
            .timeout(std::time::Duration::from_secs(15))
            .json(&body)
            .send()
            .await;

        match sent {
            Ok(response) if response.status().is_success() => mark_reported(&marker, offset),
            Ok(response) => {
                tracing::warn!(status = %response.status(), "o servidor recusou o relatório de erro")
            }
            Err(failure) => tracing::warn!(failure = %failure, "o relatório de erro não saiu"),
        }
    });
}

/// Uma linha no arquivo.
///
/// A hora vem de quem escreve: o `tracing` carimba a dele, e a interface manda a dela
/// já em ISO. `File` não tem buffer de espaço de usuário, então o `writeln` vai direto
/// para o sistema — é isso que faz a última linha sobreviver a um processo que morre
/// no meio dela.
pub fn write(line: &str) {
    let Some(file) = FILE.get() else {
        return;
    };

    let Ok(mut file) = file.lock() else {
        return;
    };

    if line.len() > MAX_LINE {
        let short: String = line.chars().take(MAX_LINE).collect();

        let _ = writeln!(file, "{short}");

        return;
    }

    let _ = writeln!(file, "{line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O caminho é o que a janela de diagnóstico mostra para a pessoa anexar. Nome fixo,
    /// dentro de uma pasta só nossa: um log solto no `HOME` ninguém acha.
    #[test]
    fn the_path_lands_on_a_named_file_inside_our_own_folder() {
        let path = path();

        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("unkvoid.log"));
        assert!(
            path.parent().is_some_and(|folder| folder.ends_with("unkvoid")),
            "log fora de uma pasta do app: {}",
            path.display()
        );
    }

    /// Linha gigante vinda da interface não pode virar arquivo gigante — e o corte não
    /// pode cair no meio de um caractere de vários bytes, que entraria em pânico dentro
    /// do próprio registrador de pânicos.
    #[test]
    fn a_huge_line_is_cut_and_stays_valid_utf8() {
        let path = std::env::temp_dir().join("unkvoid-logbook-test.log");
        let _ = std::fs::remove_file(&path);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("não abriu o arquivo de teste");

        // `write` lê do `OnceLock` global; se outro teste já o preencheu, este vira
        // apenas a checagem de que cortar não quebra.
        let _ = FILE.set(Mutex::new(file));

        write(&"ç".repeat(MAX_LINE * 2));

        let written = std::fs::read_to_string(&path).expect("não leu o arquivo de teste");
        let _ = std::fs::remove_file(&path);

        assert!(
            written.chars().count() <= MAX_LINE + 1,
            "linha não foi cortada: {} caracteres",
            written.chars().count()
        );
    }

    /// O relatório manda só o que ainda não foi mandado, e nunca o mesmo pedaço duas
    /// vezes. Errar a conta aqui é o pior dos dois mundos: ou o mesmo erro chega para
    /// sempre, ou nenhum chega nunca — e os dois parecem "funcionando" de fora.
    #[test]
    fn only_what_came_after_the_marker_is_reported() {
        let folder = std::env::temp_dir().join("unkvoid-unreported-test");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("não criou a pasta de teste");

        let log = folder.join("unkvoid.log");
        let marker = folder.join("unkvoid.sent");

        std::fs::write(&log, "primeira sessão\n").expect("não escreveu o log");

        let (slice, offset) = unreported(&log, &marker).expect("sem marcador, tudo é novo");
        assert_eq!(slice, "primeira sessão\n");

        mark_reported(&marker, offset);
        assert!(unreported(&log, &marker).is_none(), "o mesmo pedaço voltou");

        std::fs::write(&log, "primeira sessão\nsegunda sessão\n").expect("não escreveu o log");
        let (slice, offset) = unreported(&log, &marker).expect("a parte nova sumiu");
        assert_eq!(slice, "segunda sessão\n");

        // Log que recomeçou por causa do teto: o marcador ficou maior que o arquivo, e o
        // que está lá agora é todo novo.
        mark_reported(&marker, offset);
        std::fs::write(&log, "log novo\n").expect("não escreveu o log");
        let (slice, _) = unreported(&log, &marker).expect("log recomeçado não foi lido");
        assert_eq!(slice, "log novo\n");

        let _ = std::fs::remove_dir_all(&folder);
    }

    /// O nome de quem está na máquina não viaja junto, e um nome curto demais não pode
    /// picotar o resto do log.
    #[test]
    fn the_user_name_leaves_the_report() {
        let log = r"panic em C:\Users\fulano\AppData\Local\unkvoid";

        assert_eq!(
            scrub(log, "fulano"),
            r"panic em C:\Users\<usuario>\AppData\Local\unkvoid"
        );
        assert_eq!(scrub(log, "an"), log, "nome curto não pode ser trocado");
        assert_eq!(scrub(log, ""), log);
    }
}
