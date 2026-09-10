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
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Acima disto o arquivo recomeça. Log que enche o disco de quem só queria assistir a
/// uma tela é pior do que log nenhum.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Teto de uma linha vinda da interface. Ela é nossa, mas é a única entrada de fora
/// deste módulo, e um `JSON.stringify` de algo inesperado não pode virar um arquivo de
/// gigabytes.
const MAX_LINE: usize = 4_000;

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

/// Abre o arquivo, aponta o `tracing` para ele e instala o hook de pânico.
///
/// Chamado no começo do `run`, antes de qualquer coisa que possa morrer.
pub fn init() {
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
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS
    ));
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
}
