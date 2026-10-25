//! As frases que a pessoa lê quando algo dá errado. O núcleo diz o motivo; o texto é daqui,
//! sem caminho, endereço nem código de status — isso é log.

use core_app::Failure;
use core_app::api::HttpError;

/// Só o erro de validação do Laravel chega pronto para ser lido, e já fala do campo digitado.
pub fn said(error: &HttpError) -> String {
    match error {
        HttpError::Invalid { message, .. } => message.clone(),
        HttpError::Failed(failure) => sentence(*failure).to_owned(),
    }
}

/// Um motivo, uma frase. Motivo novo no núcleo para de compilar aqui — que é o ponto.
pub fn sentence(failure: Failure) -> &'static str {
    match failure {
        Failure::Unreachable => "Não deu para falar com o servidor. Confira a sua internet.",
        Failure::SignedOut => "Sua sessão terminou. Entre de novo.",
        Failure::NotAllowed => "Você não tem permissão para isso.",
        Failure::Gone => "Isso não existe mais.",
        Failure::Invalid => "O que foi digitado não serve.",
        Failure::ServerBroke => "O servidor teve um problema. Tente de novo em instantes.",
        Failure::TooFast => "Tentativas demais. Espere um pouco e tente de novo.",
        Failure::E2ee => "Não deu para proteger a chamada com criptografia. Saia e entre de novo.",
    }
}

/// A inicial do avatar sem foto.
pub fn initial(name: &str) -> String {
    name.chars().next().map(|letter| letter.to_uppercase().to_string()).unwrap_or_default()
}

/// A hora que a linha mostra: o `HH:MM` do carimbo ISO, sem data e sem fuso.
pub fn clock(stamp: &str) -> &str {
    stamp.get(11..16).unwrap_or_default()
}

/// O dia, como o Discord escreve no divisor entre dias: `12 de outubro de 2026`.
pub fn day(stamp: &str) -> String {
    const MONTHS: [&str; 12] = [
        "janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro",
        "dezembro",
    ];
    let parts: Vec<&str> = stamp.get(..10).unwrap_or_default().split('-').collect();
    let [year, month, day] = parts.as_slice() else {
        return String::new();
    };
    let month = month.parse::<usize>().ok().and_then(|month| MONTHS.get(month.wrapping_sub(1)));

    match (day.parse::<u32>(), month) {
        (Ok(day), Some(month)) => format!("{day} de {month} de {year}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_failure_reaches_the_screen_as_a_status_code_or_an_address() {
        for failure in [
            Failure::Unreachable,
            Failure::SignedOut,
            Failure::NotAllowed,
            Failure::Gone,
            Failure::Invalid,
            Failure::ServerBroke,
            Failure::TooFast,
        ] {
            let text = sentence(failure);

            assert!(!text.chars().any(|letter| letter.is_ascii_digit()), "número na tela: {text}");
            assert!(!text.contains("http") && !text.contains('/'), "endereço na tela: {text}");
        }
    }

    #[test]
    fn a_validation_message_passes_as_the_server_wrote_it() {
        let invalid = HttpError::Invalid { field: "email".into(), message: "Este e-mail já está em uso.".into() };

        assert_eq!(said(&invalid), "Este e-mail já está em uso.");
        assert_eq!(said(&HttpError::Failed(Failure::TooFast)), sentence(Failure::TooFast));
    }

    #[test]
    fn the_day_divider_reads_like_the_discord_one() {
        assert_eq!(day("2026-10-12T17:02:11-03:00"), "12 de outubro de 2026");
        assert_eq!(day("2026-01-05T00:00:00Z"), "5 de janeiro de 2026");
        assert_eq!(day("lixo"), "");
        assert_eq!(clock("2026-10-12T17:02:11-03:00"), "17:02");
        // Cortar por byte partiria o "Â" ao meio, e o avatar mostraria lixo.
        assert_eq!(initial("Ângela"), "Â");
        assert_eq!(initial(""), "");
    }
}
