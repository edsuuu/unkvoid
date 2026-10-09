//! Os clipes da pasta, do mais novo ao mais velho, e em que pasta de jogo cada um é salvo.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// O sufixo que todo clipe salvo leva: `" 2026-09-26 17-10-34"`.
const STAMP_LENGTH: usize = 20;

#[derive(Clone)]
pub struct Clip {
    pub path: PathBuf,
    /// A pasta do jogo (`ArenaBreakout`); vazia no clipe solto na pasta.
    pub game: String,
    pub title: String,
    pub date: String,
    pub details: String,
}

/// Os clipes da pasta e das pastas de jogo dentro dela (`ArenaBreakout\`, `Desktop\`), um
/// nível só: clipe salvo antes da separação por jogo, solto na pasta, continua aparecendo.
pub fn list(folder: &Path) -> Vec<Clip> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };

    let entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
    let nested: Vec<(String, std::fs::DirEntry)> = entries
        .iter()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| Some((entry.file_name().to_string_lossy().into_owned(), std::fs::read_dir(entry.path()).ok()?)))
        .flat_map(|(game, inner)| inner.flatten().map(move |entry| (game.clone(), entry)))
        .collect();

    let mut found: Vec<(SystemTime, Clip)> = entries
        .into_iter()
        .map(|entry| (String::new(), entry))
        .chain(nested)
        .filter(|(_, entry)| entry.path().extension().is_some_and(|extension| extension.eq_ignore_ascii_case("mp4")))
        .filter_map(|(game, entry)| {
            let metadata = entry.metadata().ok()?;
            let modified = metadata.modified().ok()?;
            let path = entry.path();
            let length = duration(&path).map(format_duration).unwrap_or_else(|| "—".into());

            Some((
                modified,
                Clip {
                    game,
                    title: title(&path),
                    date: display_date(modified),
                    details: format!("{length} · {}", format_size(metadata.len())),
                    path,
                },
            ))
        })
        .collect();

    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(_, clip)| clip).collect()
}

/// O nome do arquivo sem o carimbo de data, que a galeria já mostra ao lado.
fn title(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let stamped = stem.len() > STAMP_LENGTH
        && stem.is_char_boundary(stem.len() - STAMP_LENGTH)
        && stem[stem.len() - STAMP_LENGTH..].chars().all(|character| character.is_ascii_digit() || matches!(character, ' ' | '-'));

    if stamped { stem[..stem.len() - STAMP_LENGTH].to_owned() } else { stem }
}

fn duration(path: &Path) -> Option<Duration> {
    let file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();

    Some(mp4::Mp4Reader::read_header(BufReader::new(file), size).ok()?.duration())
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();

    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9).replace('.', ",")
    } else {
        format!("{} MB", bytes / 1_000_000)
    }
}

/// Onde e com que nome um clipe é salvo: a pasta do jogo e o título que vai no arquivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub folder: String,
    pub title: String,
}

impl Target {
    pub fn desktop() -> Self {
        Self { folder: "Desktop".into(), title: "Desktop".into() }
    }
}

/// Jogos reconhecidos pelo executável, com a pasta e o título de cada um. O título da janela
/// muda (o do LoL é "League of Legends (TM) Client"); o executável não.
const KNOWN_GAMES: &[(&str, &str, &str)] = &[
    ("uagame.exe", "ArenaBreakout", "Arena Breakout Infinite"),
    ("league of legends.exe", "LeagueOfLegends", "League of Legends"),
    ("leagueclientux.exe", "LeagueOfLegends", "League of Legends"),
    ("valorant-win64-shipping.exe", "Valorant", "VALORANT"),
    ("cs2.exe", "CounterStrike2", "Counter-Strike 2"),
    ("escapefromtarkov.exe", "EscapeFromTarkov", "Escape from Tarkov"),
    ("escapefromtarkovarena.exe", "EscapeFromTarkovArena", "Escape from Tarkov Arena"),
    ("tslgame.exe", "PUBG", "PUBG"),
    ("r5apex.exe", "ApexLegends", "Apex Legends"),
    ("r5apex_dx12.exe", "ApexLegends", "Apex Legends"),
    ("fortniteclient-win64-shipping.exe", "Fortnite", "Fortnite"),
    ("rainbowsix.exe", "RainbowSixSiege", "Rainbow Six Siege"),
    ("rainbowsix_be.exe", "RainbowSixSiege", "Rainbow Six Siege"),
    ("rainbowsix_vulkan.exe", "RainbowSixSiege", "Rainbow Six Siege"),
    ("gta5.exe", "GTAV", "GTA V"),
    ("gta5_enhanced.exe", "GTAV", "GTA V"),
    ("forzahorizon5.exe", "ForzaHorizon5", "Forza Horizon 5"),
    ("tlou-i.exe", "TheLastOfUsPartI", "The Last of Us Part I"),
    ("tlou-i-l.exe", "TheLastOfUsPartI", "The Last of Us Part I"),
    ("needforspeedheat.exe", "NeedForSpeedHeat", "Need for Speed Heat"),
    ("speed2.exe", "NeedForSpeedUnderground2", "Need for Speed Underground 2"),
    ("speed.exe", "NeedForSpeedMostWanted", "Need for Speed Most Wanted"),
    ("bodycam-win64-shipping.exe", "Bodycam", "Bodycam"),
    ("dota2.exe", "Dota2", "Dota 2"),
    ("overwatch.exe", "Overwatch", "Overwatch"),
    ("rocketleague.exe", "RocketLeague", "Rocket League"),
];

/// O que cobre a tela sem ser jogo: a área de trabalho, o Alt+Tab e o vídeo em tela cheia.
const NOT_GAMES: &[&str] = &["explorer.exe", "chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "discord.exe", "vlc.exe"];

/// A pasta do clipe pela janela que estava na frente no atalho. Jogo conhecido vai pelo
/// executável; outro jogo, pela janela em tela cheia, vai pelo título dela ("Hollow Knight" →
/// `HollowKnight`); fora de jogo, `Desktop`.
pub fn target(executable: &str, window_title: &str, fullscreen: bool) -> Target {
    let executable = executable.to_lowercase();

    if let Some((_, folder, title)) = KNOWN_GAMES.iter().find(|(name, _, _)| *name == executable) {
        return Target { folder: (*folder).into(), title: (*title).into() };
    }

    if NOT_GAMES.contains(&executable.as_str()) {
        return Target::desktop();
    }

    let title = clean_title(window_title);
    let folder = pascal_case(&title);

    if fullscreen && !folder.is_empty() {
        Target { folder, title }
    } else {
        Target::desktop()
    }
}

/// O nome de uma pasta de jogo na galeria: `ArenaBreakout` → "Arena Breakout Infinite".
pub fn game_label(folder: &str) -> String {
    match KNOWN_GAMES.iter().find(|(_, known, _)| *known == folder) {
        Some((_, _, title)) => (*title).into(),
        None if folder.is_empty() => "Outros".into(),
        None => folder.into(),
    }
}

/// O título sem as marcas registradas e o que vem entre parênteses ("(TM)", "(64-bit)").
fn clean_title(title: &str) -> String {
    let mut cleaned = String::new();
    let mut depth = 0_u32;

    for character in title.chars() {
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '™' | '®' | '©' => {}
            _ if depth == 0 => cleaned.push(character),
            _ => {}
        }
    }

    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "Arena Breakout Infinite" → "ArenaBreakoutInfinite": sem espaço nem caractere que o
/// Windows recuse em nome de pasta.
fn pascal_case(text: &str) -> String {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();

            characters.next().map(|first| first.to_uppercase().chain(characters).collect::<String>()).unwrap_or_default()
        })
        .collect::<String>()
        .chars()
        .take(40)
        .collect()
}

/// Um nome de arquivo com o título da janela e a hora. O que o Windows não aceita em nome de
/// arquivo sai; título vazio (a própria área de trabalho, este app) vira "Tela".
pub fn file_name(window_title: &str) -> String {
    let cleaned: String = window_title
        .chars()
        .filter(|character| !character.is_control() && !matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        .take(60)
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.');

    format!("{} {}.mp4", if cleaned.is_empty() { "Tela" } else { cleaned }, file_stamp())
}

/// "2026-09-26 17-10-34": ordena bem no Explorer e não tem caractere proibido em nome de
/// arquivo.
fn file_stamp() -> String {
    let now = unsafe { GetLocalTime() };

    format!("{:04}-{:02}-{:02} {:02}-{:02}-{:02}", now.wYear, now.wMonth, now.wDay, now.wHour, now.wMinute, now.wSecond)
}

/// "26/09/2026 17:10", no fuso da máquina.
fn display_date(time: SystemTime) -> String {
    let Ok(since_1601) = time.duration_since(SystemTime::UNIX_EPOCH).map(|since_1970| since_1970.as_nanos() / 100 + 116_444_736_000_000_000)
    else {
        return String::new();
    };
    let file_time = FILETIME { dwLowDateTime: since_1601 as u32, dwHighDateTime: (since_1601 >> 32) as u32 };
    let mut universal = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();

    unsafe {
        if FileTimeToSystemTime(&file_time, &mut universal).is_err()
            || SystemTimeToTzSpecificLocalTime(None, &universal, &mut local).is_err()
        {
            return String::new();
        }
    }

    format!("{:02}/{:02}/{:04} {:02}:{:02}", local.wDay, local.wMonth, local.wYear, local.wHour, local.wMinute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_loses_the_stamp_and_keeps_the_game() {
        assert_eq!(title(Path::new(r"C:\clips\Arena Breakout Infinite 2026-09-26 17-10-34.mp4")), "Arena Breakout Infinite");
        assert_eq!(title(Path::new(r"C:\clips\meu clipe.mp4")), "meu clipe");
    }

    #[test]
    fn each_game_gets_its_own_folder_and_the_rest_goes_to_desktop() {
        assert_eq!(target("UAGame.exe", "Arena Breakout Infinite", true).folder, "ArenaBreakout");
        assert_eq!(target("League of Legends.exe", "League of Legends (TM) Client", true).folder, "LeagueOfLegends");
        assert_eq!(target("HollowKnight.exe", "Hollow Knight™", true), Target { folder: "HollowKnight".into(), title: "Hollow Knight".into() });
        assert_eq!(target("chrome.exe", "YouTube - Google Chrome", false), Target::desktop());
        assert_eq!(target("explorer.exe", "", true), Target::desktop());
        assert_eq!(target("explorer.exe", "Program Manager", true), Target::desktop());
        assert_eq!(target("chrome.exe", "Um vídeo - YouTube - Google Chrome", true), Target::desktop());
        assert_eq!(target("x.exe", "Some Game (64-bit) [DX12]", true).folder, "SomeGame");
        assert_eq!(target("x.exe", &"palavra ".repeat(20), true).folder.chars().count(), 40);
    }

    #[test]
    fn game_folders_show_the_game_name() {
        assert_eq!(game_label("ArenaBreakout"), "Arena Breakout Infinite");
        assert_eq!(game_label("Desktop"), "Desktop");
        assert_eq!(game_label(""), "Outros");
    }

    #[test]
    fn a_window_title_becomes_a_valid_file_name() {
        let name = file_name("Tarkov: \"Raid\" <1/2>");

        assert!(name.starts_with("Tarkov Raid 12 "), "{name}");
        assert!(file_name("   ").starts_with("Tela "));
    }
}
