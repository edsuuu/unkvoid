//! Quem está falando agora, para o anel verde da voz.
//!
//! Medido no som que chega de cada pessoa. A fala acende no primeiro bloco acima do limiar
//! e só apaga 350 ms depois do último, porque o Opus nem manda pacote no silêncio: é o
//! relógio, e não o próximo pacote, que apaga o anel. Os números são os que o macOS fixou.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// O pico a partir do qual um bloco conta como fala. Vale também para o próprio microfone.
pub const LOUDNESS: f32 = 0.02;

/// Quanto a fala continua acesa depois do último pico.
const TAIL: Duration = Duration::from_millis(350);

#[derive(Debug, Default)]
pub struct Speaking {
    last_loud: HashMap<String, Instant>,
}

impl Speaking {
    /// Um bloco de som de um producer. Verdadeiro quando ele acabou de começar a falar.
    pub fn heard(&mut self, producer: &str, samples: &[f32], now: Instant) -> bool {
        let peak = samples.iter().fold(0.0_f32, |peak, sample| peak.max(sample.abs()));

        if peak <= LOUDNESS {
            return false;
        }

        self.last_loud.insert(producer.to_owned(), now).is_none()
    }

    /// Quem parou de falar: o último pico ficou mais de 350 ms para trás.
    pub fn quiet(&mut self, now: Instant) -> Vec<String> {
        let gone: Vec<String> = self
            .last_loud
            .iter()
            .filter(|(_, at)| now.saturating_duration_since(**at) >= TAIL)
            .map(|(producer, _)| producer.clone())
            .collect();

        for producer in &gone {
            self.last_loud.remove(producer);
        }

        gone
    }

    /// O producer fechou: não sobra anel aceso de quem saiu.
    pub fn forget(&mut self, producer: &str) -> bool {
        self.last_loud.remove(producer).is_some()
    }

    pub fn is_speaking(&self, producer: &str) -> bool {
        self.last_loud.contains_key(producer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOUD: [f32; 4] = [0.0, 0.3, -0.4, 0.1];
    const QUIET: [f32; 4] = [0.0, 0.01, -0.015, 0.005];

    #[test]
    fn a_loud_block_starts_the_speech_and_a_quiet_one_does_not() {
        let mut speaking = Speaking::default();
        let now = Instant::now();

        assert!(!speaking.heard("ada", &QUIET, now));
        assert!(!speaking.is_speaking("ada"));
        assert!(speaking.heard("ada", &LOUD, now));
        assert!(speaking.is_speaking("ada"));
    }

    #[test]
    fn only_the_turn_is_announced_not_every_loud_block() {
        let mut speaking = Speaking::default();
        let now = Instant::now();

        assert!(speaking.heard("ada", &LOUD, now));
        assert!(!speaking.heard("ada", &LOUD, now + Duration::from_millis(20)));
    }

    #[test]
    fn the_ring_stays_lit_through_the_tail_and_goes_out_after_it() {
        let mut speaking = Speaking::default();
        let now = Instant::now();

        speaking.heard("ada", &LOUD, now);

        assert!(speaking.quiet(now + Duration::from_millis(349)).is_empty());
        assert_eq!(speaking.quiet(now + Duration::from_millis(350)), vec!["ada".to_owned()]);
        assert!(!speaking.is_speaking("ada"));
    }

    #[test]
    fn a_new_peak_after_going_quiet_is_a_new_turn() {
        let mut speaking = Speaking::default();
        let now = Instant::now();

        speaking.heard("ada", &LOUD, now);
        speaking.quiet(now + Duration::from_secs(1));

        assert!(speaking.heard("ada", &LOUD, now + Duration::from_secs(2)));
    }

    #[test]
    fn each_person_has_their_own_clock() {
        let mut speaking = Speaking::default();
        let now = Instant::now();

        speaking.heard("ada", &LOUD, now);
        speaking.heard("bia", &LOUD, now + Duration::from_millis(300));

        assert_eq!(speaking.quiet(now + Duration::from_millis(400)), vec!["ada".to_owned()]);
        assert!(speaking.is_speaking("bia"));
    }

    #[test]
    fn whoever_left_is_forgotten() {
        let mut speaking = Speaking::default();

        speaking.heard("ada", &LOUD, Instant::now());

        assert!(speaking.forget("ada"));
        assert!(!speaking.is_speaking("ada"));
    }
}
