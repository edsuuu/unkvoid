//! Quando mostrar cada quadro: o "jitter buffer" de quem assiste.
//!
//! O app nativo mostrava cada quadro na hora em que ele ficava pronto. Numa rede com perda,
//! cada pacote pedido de novo segura o fluxo uma ida e volta, e os quadros de trás chegam todos
//! juntos — medido em 02/10 na tela do Alves: ~7 reenvios por segundo, quadros chegando em bolos
//! de 44 a 70 por segundo e só ~27 imagens por segundo na tela, aos solavancos. O WebRTC do
//! navegador, que o app em React usava, não travava porque segura a imagem o tanto que a rede
//! atrasa e mostra cada quadro no horário dele. Aqui é o mesmo.
//!
//! O horário de cada quadro vem do relógio do RTP de quem transmite. Sem relógio comum entre
//! as máquinas, o que se mede é o atraso relativo: o quanto cada quadro chegou depois do mais
//! rápido da janela recente. A espera é o maior desses atrasos — sobe na hora e desce devagar,
//! para um bolo isolado não virar um vaivém de atraso.
//!
//! Tudo é lógica pura, com o relógio passado por quem chama.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// O relógio do RTP para vídeo.
const RTP_CLOCK: f64 = 90_000.0;

/// Quanto tempo de chegadas decide a espera: o bastante para ver alguns reenvios.
const WINDOW: Duration = Duration::from_secs(3);

/// A espera nunca passa disto. Um congelamento maior que isso (a conexão de quem transmite
/// caiu um segundo) chega atrasado e é mostrado assim que fica pronto, em vez de deixar todo
/// mundo um segundo atrás para sempre.
const MOST_DELAY: f64 = 0.5;

/// Quanto a espera desce por segundo de rede calma.
const DECAY_PER_SECOND: f64 = 0.05;

/// A folga para decodificar e converter o quadro depois que ele chega.
const MARGIN: f64 = 0.01;

/// Um salto maior que isto entre o relógio do RTP e o de cá não é rede, é outro fluxo (quem
/// transmite reabriu o encoder): recomeça do zero.
const RESTART: f64 = 2.0;

#[derive(Debug, Default)]
pub struct Playout {
    /// A hora em que chegou o primeiro quadro, o zero do relógio do RTP estendido.
    anchor: Option<Instant>,
    last_timestamp: u32,
    extended: i64,
    /// Chegadas recentes: quando, e quanto cada uma estava atrás do relógio do RTP.
    arrivals: VecDeque<(Instant, f64)>,
    delay: f64,
    last_arrival: Option<Instant>,
}

impl Playout {
    /// A hora de mostrar o quadro de `timestamp`, que ficou pronto para decodificar em `now`.
    pub fn due(&mut self, timestamp: u32, now: Instant) -> Instant {
        let Some(anchor) = self.anchor else {
            self.restart(timestamp, now);

            return now;
        };

        // O relógio do RTP tem 32 bits e dá a volta a cada ~13 h; a diferença com sinal acompanha.
        self.extended += i64::from(timestamp.wrapping_sub(self.last_timestamp).cast_signed());
        self.last_timestamp = timestamp;

        #[allow(clippy::cast_precision_loss)]
        let media = self.extended as f64 / RTP_CLOCK;
        let behind = now.duration_since(anchor).as_secs_f64() - media;

        if self.arrivals.front().is_some_and(|&(_, fastest)| (behind - fastest).abs() > RESTART) {
            self.restart(timestamp, now);

            return now;
        }

        self.arrivals.push_back((now, behind));

        while self.arrivals.front().is_some_and(|&(arrived, _)| now.duration_since(arrived) > WINDOW) {
            self.arrivals.pop_front();
        }

        let fastest = self.arrivals.iter().map(|&(_, behind)| behind).fold(f64::INFINITY, f64::min);
        let slowest = self.arrivals.iter().map(|&(_, behind)| behind).fold(f64::NEG_INFINITY, f64::max);
        let spread = (slowest - fastest).min(MOST_DELAY);
        let elapsed = self.last_arrival.map_or(0.0, |last| now.duration_since(last).as_secs_f64());

        self.delay = if spread >= self.delay { spread } else { (self.delay - DECAY_PER_SECOND * elapsed).max(spread) };
        self.last_arrival = Some(now);

        anchor + Duration::from_secs_f64((fastest + media + self.delay + MARGIN).max(0.0))
    }

    /// A espera em vigor: o quanto a imagem fica atrás do quadro mais rápido.
    pub fn delay(&self) -> Duration {
        Duration::from_secs_f64(self.delay)
    }

    fn restart(&mut self, timestamp: u32, now: Instant) {
        *self = Self { anchor: Some(now), last_timestamp: timestamp, last_arrival: Some(now), ..Self::default() };
        self.arrivals.push_back((now, 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 60 quadros por segundo no relógio do RTP.
    const FRAME: u32 = 1_500;

    fn at(start: Instant, milliseconds: u64) -> Instant {
        start + Duration::from_millis(milliseconds)
    }

    #[test]
    fn a_steady_stream_is_shown_as_it_arrives() {
        let (mut playout, start) = (Playout::default(), Instant::now());

        for frame in 0..120_u32 {
            let arrived = start + Duration::from_secs_f64(f64::from(frame) / 60.0);
            let due = playout.due(frame * FRAME, arrived);

            assert!(due.duration_since(arrived) <= Duration::from_millis(11), "quadro {frame} esperou {:?}", due.duration_since(arrived));
        }

        assert!(playout.delay() < Duration::from_millis(1));
    }

    #[test]
    fn frames_that_arrive_in_a_clump_are_shown_one_interval_apart() {
        let (mut playout, start) = (Playout::default(), Instant::now());

        // Um segundo de rede calma, e depois um reenvio segura três quadros por 200 ms.
        for frame in 0..60_u32 {
            playout.due(frame * FRAME, start + Duration::from_secs_f64(f64::from(frame) / 60.0));
        }

        let held = at(start, 1_000 + 200);
        let dues: Vec<Instant> = (60..63_u32).map(|frame| playout.due(frame * FRAME, held)).collect();

        for pair in dues.windows(2) {
            let gap = pair[1].duration_since(pair[0]);

            assert!(gap >= Duration::from_millis(16) && gap <= Duration::from_millis(17), "os quadros do bolo saíram com {gap:?} entre eles");
        }

        assert!(playout.delay() >= Duration::from_millis(190), "a espera ficou em {:?}", playout.delay());
    }

    #[test]
    fn the_wait_comes_down_slowly_when_the_network_calms() {
        let (mut playout, start) = (Playout::default(), Instant::now());

        playout.due(0, start);
        playout.due(FRAME, at(start, 300));

        let after_the_clump = playout.delay();

        for frame in 2..(2 + 60 * 6_u32) {
            playout.due(frame * FRAME, start + Duration::from_secs_f64(0.3 + f64::from(frame - 1) / 60.0));
        }

        assert!(playout.delay() < after_the_clump, "a espera não desceu: {:?}", playout.delay());
    }

    #[test]
    fn the_wait_never_goes_past_half_a_second() {
        let (mut playout, start) = (Playout::default(), Instant::now());

        playout.due(0, start);
        playout.due(FRAME, at(start, 1_500));

        assert!(playout.delay() <= Duration::from_millis(500));
    }

    #[test]
    fn the_rtp_clock_turning_over_is_not_a_jump() {
        let (mut playout, start) = (Playout::default(), Instant::now());
        let first = u32::MAX - FRAME / 3;

        playout.due(first, start);

        let due = playout.due(first.wrapping_add(FRAME), start + Duration::from_secs_f64(1.0 / 60.0));

        assert!(due.duration_since(start) < Duration::from_millis(40), "a volta do relógio virou {:?}", due.duration_since(start));
    }

    #[test]
    fn a_reopened_encoder_starts_over() {
        let (mut playout, start) = (Playout::default(), Instant::now());

        playout.due(0, start);

        let reopened = at(start, 100);

        assert_eq!(playout.due(900_000_000, reopened), reopened);
    }
}
