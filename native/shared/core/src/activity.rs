//! O estado efêmero de uma atividade compartilhada e a escolha da janela que a representa.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Activity {
    pub name: String,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityCandidate {
    pub executable: String,
    pub description: Option<String>,
    pub foreground: bool,
    pub fullscreen: bool,
}

pub fn choose_activity(candidates: &[ActivityCandidate]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| {
            candidate.foreground
                && candidate.fullscreen
                && !candidate.executable.trim().is_empty()
        })
        .map(|candidate| {
            candidate
                .description
                .as_deref()
                .map(str::trim)
                .filter(|description| !description.is_empty())
                .unwrap_or_else(|| candidate.executable.trim())
                .to_owned()
        })
}

#[cfg(test)]
mod tests {
    use super::{ActivityCandidate, choose_activity};

    fn candidate(executable: &str, description: Option<&str>, foreground: bool, fullscreen: bool) -> ActivityCandidate {
        ActivityCandidate {
            executable: executable.to_owned(),
            description: description.map(str::to_owned),
            foreground,
            fullscreen,
        }
    }

    #[test]
    fn foreground_fullscreen_uses_the_executable_description() {
        let candidates = [
            candidate("unkvoid.exe", Some("Unkvoid"), true, false),
            candidate("arena.exe", Some("Arena Breakout: Infinite"), true, true),
        ];

        assert_eq!(choose_activity(&candidates).as_deref(), Some("Arena Breakout: Infinite"));
    }

    #[test]
    fn non_fullscreen_or_background_windows_are_not_shared() {
        let candidates = [
            candidate("browser.exe", Some("Browser"), true, false),
            candidate("game.exe", Some("Game"), false, true),
        ];

        assert_eq!(choose_activity(&candidates), None);
    }

    #[test]
    fn empty_description_falls_back_to_executable_name() {
        let candidates = [candidate("sample-game.exe", Some("  "), true, true)];

        assert_eq!(choose_activity(&candidates).as_deref(), Some("sample-game.exe"));
    }
}
