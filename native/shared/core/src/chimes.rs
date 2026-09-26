//! Os toques do app, os mesmos do React (`ui/core/Sounds.ts`) e do macOS: senoides de poucos
//! décimos, com ataque de 12 ms e cauda exponencial. Saem daqui prontos, em PCM, para cada
//! sistema só entregar à saída de som que a pessoa escolheu.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::models::Peer;

/// O formato da saída do núcleo: 48 kHz, estéreo intercalado.
const RATE: f64 = 48_000.0;
const VOLUME: f64 = 0.07;
const STEP_SECONDS: f64 = 0.09;
const ATTACK_SECONDS: f64 = 0.012;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Chime {
    Joined,
    Left,
    StreamStarted,
    StreamStopped,
    Message,
}

impl Chime {
    /// O toque de uma troca do elenco, na ordem do macOS: alguém entrou, alguém saiu, e só
    /// então uma tela de outra pessoa que começou ou parou. A primeira lista da sala é a
    /// chegada de quem entra, e não toca.
    pub fn after(before: &[Peer], now: &[Peer]) -> Option<Self> {
        if before.is_empty() {
            return None;
        }

        let (were, are) = (ids(before), ids(now));
        let (shown, showing) = (screens(before), screens(now));

        if are.difference(&were).next().is_some() {
            Some(Self::Joined)
        } else if were.difference(&are).next().is_some() {
            Some(Self::Left)
        } else if showing.difference(&shown).next().is_some() {
            Some(Self::StreamStarted)
        } else if shown.difference(&showing).next().is_some() {
            Some(Self::StreamStopped)
        } else {
            None
        }
    }

    /// O aviso que o React mostra junto com o toque da sala: quem entrou, quem saiu, quem
    /// começou a transmitir. Parar de transmitir só toca.
    pub fn notice_after(before: &[Peer], now: &[Peer]) -> Option<String> {
        let name_of = |peers: &[Peer], wanted: &str| {
            peers
                .iter()
                .find(|peer| peer.peer_id == wanted)
                .map_or_else(|| "alguém".to_owned(), |peer| peer.name.clone())
        };
        let (were, are) = (ids(before), ids(now));

        match Self::after(before, now)? {
            Self::Joined => are.difference(&were).next().map(|peer| format!("{} entrou", name_of(now, peer))),
            Self::Left => were.difference(&are).next().map(|peer| format!("{} saiu", name_of(before, peer))),
            Self::StreamStarted => {
                let showing = screens(now);
                let started = showing.difference(&screens(before)).next().copied()?;
                let owner = now.iter().find(|peer| peer.producers.iter().any(|producer| producer.producer_id == started))?;

                Some(format!("{} começou a transmitir", owner.name))
            }
            _ => None,
        }
    }

    /// O toque em PCM `f32`, estéreo intercalado a 48 kHz.
    pub fn samples(self) -> Vec<f32> {
        let steps = self.steps();
        let length = steps
            .iter()
            .map(|(_, starts_at, seconds)| starts_at + seconds + 0.02)
            .fold(0.0, f64::max);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let frames = (length * RATE).round() as usize;
        let mut samples = Vec::with_capacity(frames * 2);

        for frame in 0..frames {
            #[allow(clippy::cast_precision_loss)]
            let time = frame as f64 / RATE;
            let mut sample = 0.0;

            for &(hertz, starts_at, seconds) in steps {
                let elapsed = time - starts_at;

                if elapsed < 0.0 || elapsed >= seconds {
                    continue;
                }

                // A rampa linear e a exponencial do `GainNode` do React.
                let envelope = if elapsed < ATTACK_SECONDS {
                    elapsed / ATTACK_SECONDS
                } else {
                    (0.0001 / VOLUME).powf((elapsed - ATTACK_SECONDS) / (seconds - ATTACK_SECONDS))
                };

                sample += (std::f64::consts::TAU * hertz * time).sin() * VOLUME * envelope;
            }

            #[allow(clippy::cast_possible_truncation)]
            samples.extend([sample as f32, sample as f32]);
        }

        samples
    }

    /// Cada nota: a frequência, quando começa e quanto dura.
    fn steps(self) -> &'static [(f64, f64, f64)] {
        match self {
            Self::Joined => &[(523.0, 0.0, STEP_SECONDS), (784.0, 0.08, STEP_SECONDS)],
            Self::Left => &[(659.0, 0.0, STEP_SECONDS), (440.0, 0.08, STEP_SECONDS)],
            Self::StreamStarted => &[(587.0, 0.0, STEP_SECONDS), (740.0, 0.08, STEP_SECONDS), (880.0, 0.16, STEP_SECONDS)],
            Self::StreamStopped => &[(880.0, 0.0, STEP_SECONDS), (740.0, 0.08, STEP_SECONDS), (587.0, 0.16, STEP_SECONDS)],
            Self::Message => &[(988.0, 0.0, 0.06), (1319.0, 0.05, 0.1)],
        }
    }
}

fn ids(peers: &[Peer]) -> HashSet<&str> {
    peers.iter().map(|peer| peer.peer_id.as_str()).collect()
}

/// As telas das outras pessoas: a própria não toca para quem a liga.
fn screens(peers: &[Peer]) -> HashSet<&str> {
    peers
        .iter()
        .filter(|peer| !peer.self_peer)
        .flat_map(|peer| &peer.producers)
        .filter(|producer| producer.source == "screen")
        .map(|producer| producer.producer_id.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ProducerInfo;

    fn peer(id: &str, screens: &[&str], self_peer: bool) -> Peer {
        Peer {
            peer_id: id.into(),
            user_id: None,
            name: id.into(),
            producers: screens
                .iter()
                .map(|screen| ProducerInfo {
                    producer_id: (*screen).into(),
                    kind: "video".into(),
                    source: "screen".into(),
                    paused: false,
                })
                .collect(),
            reconnecting: false,
            self_peer,
        }
    }

    #[test]
    fn the_first_list_is_arriving_and_does_not_chime() {
        assert_eq!(Chime::after(&[], &[peer("eu", &[], true), peer("ana", &[], false)]), None);
    }

    #[test]
    fn someone_arriving_or_leaving_chimes_before_any_screen() {
        let alone = [peer("eu", &[], true)];
        let with_ana = [peer("eu", &[], true), peer("ana", &["tela"], false)];

        assert_eq!(Chime::after(&alone, &with_ana), Some(Chime::Joined));
        assert_eq!(Chime::after(&with_ana, &alone), Some(Chime::Left));
    }

    #[test]
    fn a_screen_of_someone_else_starting_or_stopping_chimes() {
        let quiet = [peer("eu", &[], true), peer("ana", &[], false)];
        let sharing = [peer("eu", &[], true), peer("ana", &["tela"], false)];

        assert_eq!(Chime::after(&quiet, &sharing), Some(Chime::StreamStarted));
        assert_eq!(Chime::after(&sharing, &quiet), Some(Chime::StreamStopped));
        assert_eq!(Chime::after(&quiet, &quiet), None);
    }

    #[test]
    fn my_own_screen_does_not_chime_for_me() {
        let quiet = [peer("eu", &[], true), peer("ana", &[], false)];
        let mine = [peer("eu", &["minha"], true), peer("ana", &[], false)];

        assert_eq!(Chime::after(&quiet, &mine), None);
    }

    #[test]
    fn the_room_says_who_came_left_or_started_sharing() {
        let alone = [peer("eu", &[], true)];
        let with_ana = [peer("eu", &[], true), peer("ana", &[], false)];
        let ana_sharing = [peer("eu", &[], true), peer("ana", &["tela"], false)];

        assert_eq!(Chime::notice_after(&alone, &with_ana).as_deref(), Some("ana entrou"));
        assert_eq!(Chime::notice_after(&with_ana, &alone).as_deref(), Some("ana saiu"));
        assert_eq!(Chime::notice_after(&with_ana, &ana_sharing).as_deref(), Some("ana começou a transmitir"));
        assert_eq!(Chime::notice_after(&ana_sharing, &with_ana), None);
    }

    #[test]
    fn a_chime_is_as_long_as_its_notes_and_as_soft_as_the_react_one() {
        let samples = Chime::Joined.samples();
        let loudest = samples.iter().fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()));

        // 0,08 + 0,09 + 0,02 s em estéreo a 48 kHz.
        assert_eq!(samples.len(), 9_120 * 2);
        assert!(loudest > 0.05 && loudest <= 0.14, "o pico saiu {loudest}");
        assert_eq!(samples[0], 0.0, "o ataque começa do silêncio");
    }

    #[test]
    fn the_chime_names_are_the_ones_the_interfaces_read() {
        assert_eq!(serde_json::to_value(Chime::StreamStarted).unwrap(), "streamStarted");
        assert_eq!(serde_json::from_value::<Chime>("left".into()).unwrap(), Chime::Left);
    }
}
