//! O código que **é** a sala.
//!
//! Não existe em banco nenhum: quem tem o código entra, e a sala some quando o último sai.
//! Por isso a regra do que é um código válido mora aqui e não em cada interface — três
//! alfabetos diferentes seriam três salas que não se encontram.

use rand::Rng;

/// O tamanho que o app gera. Aceitar menos é para quem digita um código à mão.
pub const LENGTH: usize = 12;

pub const MIN_LENGTH: usize = 3;

pub const MAX_LENGTH: usize = 32;

/// Sem maiúscula e sem acento: o código é ditado por voz e digitado errado o tempo todo.
const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

pub fn generate() -> String {
    let mut rng = rand::thread_rng();

    (0..LENGTH)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

/// Hífen no meio é permitido para quem quiser um código legível ("time-da-tarde"); no
/// começo ou no fim, não — colar de um chat traz hífen de sobra.
pub fn is_valid(code: &str) -> bool {
    let length = code.chars().count();

    if !(MIN_LENGTH..=MAX_LENGTH).contains(&length) {
        return false;
    }

    if code.starts_with('-') || code.ends_with('-') {
        return false;
    }

    code.chars()
        .all(|letter| letter.is_ascii_lowercase() || letter.is_ascii_digit() || letter == '-')
}

/// O que se aceita de quem digitou: espaço sobrando e maiúscula não são erro de código.
pub fn clean(typed: &str) -> String {
    typed.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_code_is_always_valid() {
        for _ in 0..200 {
            let code = generate();

            assert_eq!(code.len(), LENGTH);
            assert!(is_valid(&code), "generated an invalid code: {code}");
        }
    }

    #[test]
    fn two_codes_in_a_row_are_not_the_same() {
        assert_ne!(generate(), generate());
    }

    #[test]
    fn rejects_what_would_split_a_room_in_two() {
        assert!(!is_valid(""));
        assert!(!is_valid("ab"));
        assert!(!is_valid(&"a".repeat(MAX_LENGTH + 1)));
        assert!(!is_valid("-abc"));
        assert!(!is_valid("abc-"));
        assert!(!is_valid("ABC123"), "uppercase would be a different room");
        assert!(!is_valid("sala do edsu"));
        assert!(!is_valid("salão"));
    }

    #[test]
    fn accepts_a_readable_code() {
        assert!(is_valid("time-da-tarde"));
        assert!(is_valid("abc"));
    }

    #[test]
    fn cleaning_makes_what_was_pasted_usable() {
        assert_eq!(clean("  ABC-123 \n"), "abc-123");
        assert!(is_valid(&clean(" TIME-da-Tarde ")));
    }
}
