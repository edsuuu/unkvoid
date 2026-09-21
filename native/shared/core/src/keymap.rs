//! O código da tecla que a interface grava, traduzido para o número que cada sistema usa.
//!
//! A interface grava `KeyM`, `F13`, `Space` — o mesmo nome em qualquer sistema, porque é o
//! que o navegador e o teclado virtual dão. Quem consulta o teclado quer um número, e o
//! número é diferente em cada plataforma.
//!
//! **Por que consultar e não registrar.** Registrar o atalho no sistema (`RegisterHotKey`
//! no Windows, `RegisterEventHotKey` no macOS) faz o sistema *engolir* a tecla: ela não
//! chega a quem está na frente. Num app para quem joga, isso é inaceitável — apertar a
//! tecla de mutar não pode cancelar o movimento do personagem. Por isso a tecla é lida, em
//! volta de 20 ms, e nunca capturada.

/// O acelerador como a interface o escreve, quebrado em tecla e modificadores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accelerator<'a> {
    pub key: &'a str,
    pub modifiers: Vec<&'a str>,
}

pub fn split(accelerator: &str) -> Option<Accelerator<'_>> {
    let mut parts: Vec<&str> = accelerator
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    let key = parts.pop()?;

    Some(Accelerator {
        key,
        modifiers: parts,
    })
}

/// O número que o macOS usa (`kVK_*` do Carbon), que não se parece com o do Windows nem
/// com a posição da tecla no teclado.
#[cfg(target_os = "macos")]
pub fn macos_key(code: &str) -> Option<u16> {
    if let Some(letter) = code
        .strip_prefix("Key")
        .and_then(|rest| rest.chars().next())
    {
        return macos_letter(letter);
    }

    if let Some(digit) = code
        .strip_prefix("Digit")
        .and_then(|rest| rest.parse::<u8>().ok())
    {
        return macos_digit(digit);
    }

    if let Some(number) = code
        .strip_prefix('F')
        .and_then(|rest| rest.parse::<u8>().ok())
    {
        return macos_function(number);
    }

    Some(match code {
        "Space" => 49,
        "Enter" => 36,
        "Tab" => 48,
        "Backspace" => 51,
        "Escape" => 53,
        "ArrowLeft" => 123,
        "ArrowRight" => 124,
        "ArrowDown" => 125,
        "ArrowUp" => 126,
        "Home" => 115,
        "End" => 119,
        "PageUp" => 116,
        "PageDown" => 121,
        "Delete" => 117,
        "Minus" => 27,
        "Equal" => 24,
        "BracketLeft" => 33,
        "BracketRight" => 30,
        "Backslash" => 42,
        "Semicolon" => 41,
        "Quote" => 39,
        "Comma" => 43,
        "Period" => 47,
        "Slash" => 44,
        "Backquote" => 50,
        _ => return None,
    })
}

/// O caminho de volta: o número que o sistema deu quando a pessoa apertou a tecla, no nome
/// que a interface grava. É procurar em `macos_key`, para as duas direções nunca divergirem.
#[cfg(target_os = "macos")]
pub fn macos_key_name(code: u16) -> Option<String> {
    const NAMED: [&str; 24] = [
        "Space",
        "Enter",
        "Tab",
        "Backspace",
        "Escape",
        "ArrowLeft",
        "ArrowRight",
        "ArrowDown",
        "ArrowUp",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Delete",
        "Minus",
        "Equal",
        "BracketLeft",
        "BracketRight",
        "Backslash",
        "Semicolon",
        "Quote",
        "Comma",
        "Period",
        "Slash",
    ];

    let letters = ('A'..='Z').map(|letter| format!("Key{letter}"));
    let digits = (0..=9).map(|digit| format!("Digit{digit}"));
    let functions = (1..=20).map(|number| format!("F{number}"));
    let named = NAMED
        .iter()
        .map(|name| (*name).to_owned())
        .chain(["Backquote".to_owned()]);

    letters
        .chain(digits)
        .chain(functions)
        .chain(named)
        .find(|name| macos_key(name) == Some(code))
}

#[cfg(target_os = "macos")]
fn macos_letter(letter: char) -> Option<u16> {
    const LETTERS: [(char, u16); 26] = [
        ('a', 0),
        ('b', 11),
        ('c', 8),
        ('d', 2),
        ('e', 14),
        ('f', 3),
        ('g', 5),
        ('h', 4),
        ('i', 34),
        ('j', 38),
        ('k', 40),
        ('l', 37),
        ('m', 46),
        ('n', 45),
        ('o', 31),
        ('p', 35),
        ('q', 12),
        ('r', 15),
        ('s', 1),
        ('t', 17),
        ('u', 32),
        ('v', 9),
        ('w', 13),
        ('x', 7),
        ('y', 16),
        ('z', 6),
    ];

    let letter = letter.to_ascii_lowercase();

    LETTERS
        .iter()
        .find(|(known, _)| *known == letter)
        .map(|(_, code)| *code)
}

#[cfg(target_os = "macos")]
fn macos_digit(digit: u8) -> Option<u16> {
    const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];

    DIGITS.get(usize::from(digit)).copied()
}

#[cfg(target_os = "macos")]
fn macos_function(number: u8) -> Option<u16> {
    const FUNCTIONS: [(u8, u16); 20] = [
        (1, 122),
        (2, 120),
        (3, 99),
        (4, 118),
        (5, 96),
        (6, 97),
        (7, 98),
        (8, 100),
        (9, 101),
        (10, 109),
        (11, 103),
        (12, 111),
        (13, 105),
        (14, 107),
        (15, 113),
        (16, 106),
        (17, 64),
        (18, 79),
        (19, 80),
        (20, 90),
    ];

    FUNCTIONS
        .iter()
        .find(|(known, _)| *known == number)
        .map(|(_, code)| *code)
}

/// Cada modificador vira as teclas que servem: a pessoa segura o Shift da esquerda ou o da
/// direita, e os dois valem.
#[cfg(target_os = "macos")]
pub fn macos_modifier(name: &str) -> Option<&'static [u16]> {
    const SHIFT: &[u16] = &[56, 60];
    const CONTROL: &[u16] = &[59, 62];
    const OPTION: &[u16] = &[58, 61];
    const COMMAND: &[u16] = &[55, 54];

    Some(match name {
        "Shift" => SHIFT,
        "Control" | "Ctrl" => CONTROL,
        "Alt" | "Option" => OPTION,
        // No macOS o atalho natural é o Command; `CmdOrCtrl` é como a interface escreve o
        // "o que for de casa nesta plataforma".
        "Meta" | "Command" | "Cmd" | "Super" | "CmdOrCtrl" => COMMAND,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn every_key_that_can_be_saved_can_be_read_back() {
        for name in ["KeyM", "Digit0", "F13", "Space", "Backquote", "ArrowUp"] {
            let code = macos_key(name).expect(name);

            assert_eq!(macos_key_name(code).as_deref(), Some(name));
        }

        assert_eq!(macos_key_name(9_999), None);
    }

    #[test]
    fn an_accelerator_splits_into_key_and_modifiers() {
        let parsed = split("CmdOrCtrl+Shift+KeyM").expect("split");

        assert_eq!(parsed.key, "KeyM");
        assert_eq!(parsed.modifiers, vec!["CmdOrCtrl", "Shift"]);
    }

    #[test]
    fn a_bare_key_has_no_modifiers() {
        let parsed = split("F13").expect("split");

        assert_eq!(parsed.key, "F13");
        assert!(parsed.modifiers.is_empty());
    }

    #[test]
    fn an_empty_accelerator_is_refused() {
        assert!(split("").is_none());
        assert!(split("  ").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_letter_and_digit_has_a_macos_code() {
        for letter in 'a'..='z' {
            assert!(
                macos_key(&format!("Key{}", letter.to_ascii_uppercase())).is_some(),
                "faltou {letter}"
            );
        }

        for digit in 0..=9 {
            assert!(
                macos_key(&format!("Digit{digit}")).is_some(),
                "faltou o dígito {digit}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn no_two_keys_share_a_code() {
        let mut codes = Vec::new();

        for letter in 'a'..='z' {
            codes.push(macos_key(&format!("Key{letter}")).expect("letter"));
        }

        for digit in 0..=9 {
            codes.push(macos_key(&format!("Digit{digit}")).expect("digit"));
        }

        for name in ["Space", "Enter", "Tab", "Escape", "ArrowUp", "Home"] {
            codes.push(macos_key(name).expect("named"));
        }

        let unique: std::collections::HashSet<_> = codes.iter().collect();

        assert_eq!(
            unique.len(),
            codes.len(),
            "há teclas diferentes com o mesmo código"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_default_binds_of_the_app_all_translate() {
        // São as três que vêm de fábrica em Voice.DEFAULT_KEYBINDS.
        for accelerator in ["CmdOrCtrl+Shift+KeyM", "CmdOrCtrl+Shift+KeyD", "F13"] {
            let parsed = split(accelerator).expect("split");

            assert!(macos_key(parsed.key).is_some(), "sem código: {accelerator}");

            for modifier in parsed.modifiers {
                assert!(
                    macos_modifier(modifier).is_some(),
                    "sem modificador: {modifier}"
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn an_unknown_key_is_refused_instead_of_guessed() {
        assert!(macos_key("Teclado").is_none());
        assert!(macos_key("F99").is_none());
        assert!(macos_modifier("Hyper").is_none());
    }
}
