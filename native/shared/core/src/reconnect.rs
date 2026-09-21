//! Quanto esperar antes de tentar de novo.
//!
//! O jitter não é enfeite: num restart do SFU todo mundo cai no mesmo instante, e sem ele
//! todos voltariam juntos, na mesma fração de segundo, contra um servidor que acabou de
//! subir — e que ainda tem teto de conexões novas por IP. Espalhar as tentativas é o que
//! faz a volta funcionar.
//!
//! É o mesmo desenho do `SfuClient.ts`, para o app nativo e o app de hoje se comportarem
//! igual diante da mesma queda.

use std::time::Duration;

use rand::Rng;

/// Depois disto o app desiste e avisa quem está olhando a tela.
pub const MAX_ATTEMPTS: u32 = 8;

const FIRST_MS: u64 = 1_000;

const CEILING_MS: u64 = 10_000;

#[derive(Debug, Default)]
pub struct Backoff {
    pub attempt: u32,
}

impl Backoff {
    /// `None` quando não vale mais tentar.
    pub fn next_delay(&mut self) -> Option<Duration> {
        if self.attempt >= MAX_ATTEMPTS {
            return None;
        }

        let ceiling = FIRST_MS
            .saturating_mul(1_u64 << self.attempt)
            .min(CEILING_MS);
        let half = ceiling / 2;

        self.attempt += 1;

        // Metade fixa e metade sorteada: garante um mínimo de espera e ainda assim
        // espalha as voltas.
        Some(Duration::from_millis(
            half + rand::thread_rng().gen_range(0..=half),
        ))
    }

    /// Uma conexão que deu certo zera a conta.
    pub fn succeeded(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delay_grows_and_stops_at_the_ceiling() {
        let mut backoff = Backoff::default();
        let mut delays = Vec::new();

        while let Some(delay) = backoff.next_delay() {
            delays.push(delay.as_millis() as u64);
        }

        assert_eq!(
            delays.len(),
            MAX_ATTEMPTS as usize,
            "gave up at the wrong point"
        );

        // A primeira espera fica entre meio segundo e um segundo.
        assert!(
            (500..=1_000).contains(&delays[0]),
            "first delay: {}",
            delays[0]
        );

        // Nenhuma passa do teto, mesmo na oitava tentativa.
        assert!(
            delays.iter().all(|delay| *delay <= CEILING_MS),
            "above the ceiling: {delays:?}"
        );

        // A última é bem maior que a primeira: a curva subiu de verdade.
        assert!(delays.last().unwrap() > &delays[0]);
    }

    #[test]
    fn gives_up_after_the_limit() {
        let mut backoff = Backoff::default();

        for _ in 0..MAX_ATTEMPTS {
            assert!(backoff.next_delay().is_some());
        }

        assert!(backoff.next_delay().is_none());
        assert!(backoff.next_delay().is_none());
    }

    #[test]
    fn a_successful_connection_resets_the_count() {
        let mut backoff = Backoff::default();

        backoff.next_delay();
        backoff.next_delay();

        assert_eq!(backoff.attempt, 2);

        backoff.succeeded();

        assert_eq!(backoff.attempt, 0);
        assert!(backoff.next_delay().is_some());
    }

    #[test]
    fn two_machines_do_not_come_back_at_the_same_instant() {
        let delays: Vec<u64> = (0..50)
            .map(|_| {
                Backoff::default()
                    .next_delay()
                    .expect("first delay")
                    .as_millis() as u64
            })
            .collect();

        let distinct: std::collections::HashSet<_> = delays.iter().collect();

        assert!(
            distinct.len() > 1,
            "every delay came out the same: no jitter"
        );
    }
}
