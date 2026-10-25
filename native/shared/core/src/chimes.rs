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
    Muted,
    Unmuted,
    Deafened,
    Undeafened,
    CameraOn,
    CameraOff,
    Disconnected,
    TalkStarted,
    TalkStopped,
    Moved,
    ViewerJoined,
    ViewerLeft,
}

/// Onde a escolha de cada som fica: um objeto com o nome do som e `false` para o desligado.
pub const SOUNDS_KEY: &str = "unkvoid:sounds";

impl Chime {
    /// Todos os sons, na ordem em que a tela de Sons os lista.
    pub const ALL: [Self; 17] = [
        Self::Deafened,
        Self::Undeafened,
        Self::Muted,
        Self::Unmuted,
        Self::Disconnected,
        Self::TalkStarted,
        Self::TalkStopped,
        Self::Joined,
        Self::Left,
        Self::Moved,
        Self::StreamStarted,
        Self::StreamStopped,
        Self::ViewerJoined,
        Self::ViewerLeft,
        Self::CameraOn,
        Self::CameraOff,
        Self::Message,
    ];

    /// O nome que o som tem no protocolo e na preferência (`muted`, `streamStarted`…).
    pub fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    /// O som está ligado na preferência guardada? Todos nascem ligados, como no Discord: só o
    /// `false` explícito cala um.
    pub fn allowed(self, sounds: Option<&serde_json::Value>) -> bool {
        sounds
            .and_then(|sounds| sounds[self.name()].as_bool())
            .unwrap_or(true)
    }

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
            Self::Joined => are
                .difference(&were)
                .next()
                .map(|peer| format!("{} entrou", name_of(now, peer))),
            Self::Left => were
                .difference(&are)
                .next()
                .map(|peer| format!("{} saiu", name_of(before, peer))),
            Self::StreamStarted => {
                let showing = screens(now);
                let started = showing.difference(&screens(before)).next().copied()?;
                let owner = now.iter().find(|peer| {
                    peer.producers
                        .iter()
                        .any(|producer| producer.producer_id == started)
                })?;

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
            Self::StreamStarted => &[
                (587.0, 0.0, STEP_SECONDS),
                (740.0, 0.08, STEP_SECONDS),
                (880.0, 0.16, STEP_SECONDS),
            ],
            Self::StreamStopped => &[
                (880.0, 0.0, STEP_SECONDS),
                (740.0, 0.08, STEP_SECONDS),
                (587.0, 0.16, STEP_SECONDS),
            ],
            Self::Message => &[(988.0, 0.0, 0.06), (1319.0, 0.05, 0.1)],
            Self::Muted => &[(740.0, 0.0, STEP_SECONDS), (494.0, 0.07, STEP_SECONDS)],
            Self::Unmuted => &[(494.0, 0.0, STEP_SECONDS), (740.0, 0.07, STEP_SECONDS)],
            Self::Deafened => &[(440.0, 0.0, STEP_SECONDS), (293.0, 0.07, STEP_SECONDS)],
            Self::Undeafened => &[(293.0, 0.0, STEP_SECONDS), (440.0, 0.07, STEP_SECONDS)],
            Self::CameraOn => &[(587.0, 0.0, STEP_SECONDS), (784.0, 0.07, STEP_SECONDS)],
            Self::CameraOff => &[(784.0, 0.0, STEP_SECONDS), (587.0, 0.07, STEP_SECONDS)],
            Self::Disconnected => &[(392.0, 0.0, 0.07), (293.0, 0.05, 0.07), (196.0, 0.10, 0.07)],
            Self::TalkStarted => &[(880.0, 0.0, 0.02), (1175.0, 0.02, 0.02)],
            Self::TalkStopped => &[(1175.0, 0.0, 0.02), (880.0, 0.02, 0.02)],
            Self::Moved => &[(659.0, 0.0, 0.07), (523.0, 0.06, 0.07), (392.0, 0.12, 0.07)],
            Self::ViewerJoined => &[(523.0, 0.0, STEP_SECONDS), (659.0, 0.07, STEP_SECONDS)],
            Self::ViewerLeft => &[(659.0, 0.0, STEP_SECONDS), (523.0, 0.07, STEP_SECONDS)],
        }
    }
}

/// A altura dos efeitos do soundboard; os toques ficam em 0,055.
const BOARD_AMPLITUDE: f64 = 0.16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundboardSound {
    Pulse,
    Spark,
    Orbit,
    Breeze,
    Prism,
    Echo,
    Aurora,
    Comet,
}

impl SoundboardSound {
    pub const ALL: [Self; 8] = [
        Self::Pulse,
        Self::Spark,
        Self::Orbit,
        Self::Breeze,
        Self::Prism,
        Self::Echo,
        Self::Aurora,
        Self::Comet,
    ];

    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            Self::Pulse => ("Pulso", "🔊"),
            Self::Spark => ("Faísca", "✨"),
            Self::Orbit => ("Órbita", "🪐"),
            Self::Breeze => ("Brisa", "🍃"),
            Self::Prism => ("Prisma", "🔷"),
            Self::Echo => ("Eco", "🌀"),
            Self::Aurora => ("Aurora", "🌌"),
            Self::Comet => ("Cometa", "☄️"),
        }
    }

    pub fn samples(self) -> Vec<f32> {
        let steps = self.steps();
        let frames = (steps
            .iter()
            .map(|(_, start, seconds)| start + seconds + 0.02)
            .fold(0.0, f64::max)
            * RATE)
            .round() as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for frame in 0..frames {
            let time = frame as f64 / RATE;
            let mut value = 0.0;
            for &(frequency, start, duration) in steps {
                let elapsed = time - start;
                if !(0.0..duration).contains(&elapsed) {
                    continue;
                }
                let envelope = if elapsed < ATTACK_SECONDS {
                    elapsed / ATTACK_SECONDS
                } else {
                    (0.0001_f64 / BOARD_AMPLITUDE).powf((elapsed - ATTACK_SECONDS) / (duration - ATTACK_SECONDS))
                };
                // Mais alto que os toques: o efeito entra na voz e precisa aparecer por cima dela.
                value += (std::f64::consts::TAU * frequency * time).sin() * BOARD_AMPLITUDE * envelope;
            }
            let value = value.clamp(-1.0, 1.0) as f32;
            samples.extend([value, value]);
        }
        samples
    }

    fn steps(self) -> &'static [(f64, f64, f64)] {
        match self {
            Self::Pulse => &[(392.0, 0.0, 0.14), (523.0, 0.13, 0.14), (659.0, 0.26, 0.16)],
            Self::Spark => &[
                (1047.0, 0.0, 0.10),
                (1397.0, 0.09, 0.12),
                (1760.0, 0.18, 0.15),
            ],
            Self::Orbit => &[(262.0, 0.0, 0.18), (392.0, 0.16, 0.18), (587.0, 0.32, 0.18)],
            Self::Breeze => &[(784.0, 0.0, 0.16), (698.0, 0.12, 0.18), (587.0, 0.24, 0.20)],
            Self::Prism => &[(440.0, 0.0, 0.12), (554.0, 0.08, 0.14), (880.0, 0.16, 0.16)],
            Self::Echo => &[(330.0, 0.0, 0.20), (494.0, 0.18, 0.18), (330.0, 0.34, 0.20)],
            Self::Aurora => &[
                (587.0, 0.0, 0.18),
                (880.0, 0.16, 0.20),
                (1175.0, 0.32, 0.22),
            ],
            Self::Comet => &[
                (1319.0, 0.0, 0.08),
                (988.0, 0.07, 0.11),
                (659.0, 0.16, 0.18),
            ],
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

    #[test]
    fn soundboard_sounds_are_distinct_and_shorter_than_two_seconds() {
        let sounds: Vec<_> = SoundboardSound::ALL
            .into_iter()
            .map(SoundboardSound::samples)
            .collect();
        for (index, sound) in sounds.iter().enumerate() {
            // Entre 0,25 s e 2 s, em estéreo a 48 kHz.
            assert!(sound.len() >= 24_000 && sound.len() <= 192_000);
            assert!(sound.iter().all(|sample| sample.abs() <= 1.0));
            assert!(sounds[..index].iter().all(|previous| previous != sound));
        }
    }

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
                    e2ee_member_id: None,
                })
                .collect(),
            reconnecting: false,
            self_peer,
        }
    }

    #[test]
    fn the_first_list_is_arriving_and_does_not_chime() {
        assert_eq!(
            Chime::after(&[], &[peer("eu", &[], true), peer("ana", &[], false)]),
            None
        );
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

        assert_eq!(
            Chime::notice_after(&alone, &with_ana).as_deref(),
            Some("ana entrou")
        );
        assert_eq!(
            Chime::notice_after(&with_ana, &alone).as_deref(),
            Some("ana saiu")
        );
        assert_eq!(
            Chime::notice_after(&with_ana, &ana_sharing).as_deref(),
            Some("ana começou a transmitir")
        );
        assert_eq!(Chime::notice_after(&ana_sharing, &with_ana), None);
    }

    #[test]
    fn a_chime_is_as_long_as_its_notes_and_as_soft_as_the_react_one() {
        let samples = Chime::Joined.samples();
        let loudest = samples
            .iter()
            .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()));

        // 0,08 + 0,09 + 0,02 s em estéreo a 48 kHz.
        assert_eq!(samples.len(), 9_120 * 2);
        assert!(loudest > 0.05 && loudest <= 0.14, "o pico saiu {loudest}");
        assert_eq!(samples[0], 0.0, "o ataque começa do silêncio");
    }

    #[test]
    fn the_chime_names_are_the_ones_the_interfaces_read() {
        assert_eq!(
            serde_json::to_value(Chime::StreamStarted).unwrap(),
            "streamStarted"
        );
        assert_eq!(
            serde_json::from_value::<Chime>("left".into()).unwrap(),
            Chime::Left
        );
    }

    #[test]
    fn every_chime_has_a_reasonable_duration_and_never_clips() {
        let chimes = [
            Chime::Joined,
            Chime::Left,
            Chime::StreamStarted,
            Chime::StreamStopped,
            Chime::Message,
            Chime::Muted,
            Chime::Unmuted,
            Chime::Deafened,
            Chime::Undeafened,
            Chime::CameraOn,
            Chime::CameraOff,
            Chime::Disconnected,
            Chime::TalkStarted,
            Chime::TalkStopped,
            Chime::Moved,
            Chime::ViewerJoined,
            Chime::ViewerLeft,
        ];

        for chime in chimes {
            let samples = chime.samples();
            let seconds = samples.len() as f64 / 2.0 / RATE;
            let peak = samples
                .iter()
                .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));

            assert!((0.04..=0.4).contains(&seconds), "{chime:?} dura {seconds}s");
            assert!(peak <= 1.0, "{chime:?} estoura com pico {peak}");
        }
    }

    #[test]
    fn paired_chimes_are_mirrored_rising_and_falling_sequences() {
        let pairs = [
            (Chime::Muted, Chime::Unmuted),
            (Chime::Deafened, Chime::Undeafened),
            (Chime::CameraOff, Chime::CameraOn),
            (Chime::TalkStopped, Chime::TalkStarted),
            (Chime::ViewerLeft, Chime::ViewerJoined),
        ];

        for (falling, rising) in pairs {
            let falling_steps = falling.steps();
            let rising_steps = rising.steps();
            let falling_frequencies: Vec<_> = falling_steps.iter().map(|step| step.0).collect();
            let rising_frequencies: Vec<_> = rising_steps.iter().rev().map(|step| step.0).collect();

            assert_eq!(
                falling_frequencies, rising_frequencies,
                "{falling:?}/{rising:?}"
            );
            assert!(falling_steps.first().unwrap().0 > falling_steps.last().unwrap().0);
        }
    }

    #[test]
    fn deafening_uses_a_lower_register_than_muting_and_ptt_stays_short() {
        for (deafened, muted) in Chime::Deafened.steps().iter().zip(Chime::Muted.steps()) {
            assert!(deafened.0 < muted.0);
        }
        for chime in [Chime::TalkStarted, Chime::TalkStopped] {
            let seconds = chime.samples().len() as f64 / 2.0 / RATE;
            assert!(
                (0.05..=0.07).contains(&seconds),
                "{chime:?} dura {seconds}s"
            );
        }
    }

    #[test]
    fn every_sound_starts_on_and_only_an_explicit_false_turns_it_off() {
        let saved = serde_json::json!({ "muted": false, "unmuted": true, "message": "lixo" });

        assert!(Chime::Muted.allowed(None), "sem nada guardado, tudo toca");
        assert!(!Chime::Muted.allowed(Some(&saved)));
        assert!(Chime::Unmuted.allowed(Some(&saved)));
        assert!(
            Chime::Message.allowed(Some(&saved)),
            "valor que não é booleano não cala"
        );
        assert_eq!(
            Chime::ALL.len(),
            Chime::ALL
                .iter()
                .map(|chime| chime.name())
                .collect::<HashSet<_>>()
                .len(),
            "a lista não repete som"
        );
    }

    /// Dois toques iguais não dizem o que aconteceu: desmutar soava como alguém entrando.
    #[test]
    fn no_two_chimes_sound_the_same() {
        let chimes = [
            Chime::Joined,
            Chime::Left,
            Chime::StreamStarted,
            Chime::StreamStopped,
            Chime::Message,
            Chime::Muted,
            Chime::Unmuted,
            Chime::Deafened,
            Chime::Undeafened,
            Chime::CameraOn,
            Chime::CameraOff,
            Chime::Disconnected,
            Chime::TalkStarted,
            Chime::TalkStopped,
            Chime::Moved,
            Chime::ViewerJoined,
            Chime::ViewerLeft,
        ];
        let notes = |chime: &Chime| {
            chime
                .steps()
                .iter()
                .map(|step| step.0 as u32)
                .collect::<Vec<_>>()
        };

        for (index, chime) in chimes.iter().enumerate() {
            for other in &chimes[index + 1..] {
                assert_ne!(
                    notes(chime),
                    notes(other),
                    "{chime:?} e {other:?} tocam as mesmas notas"
                );
            }
        }
    }

    #[test]
    fn all_chime_names_use_the_expected_camel_case_protocol_names() {
        let names = [
            (Chime::Joined, "joined"),
            (Chime::Left, "left"),
            (Chime::StreamStarted, "streamStarted"),
            (Chime::StreamStopped, "streamStopped"),
            (Chime::Message, "message"),
            (Chime::Muted, "muted"),
            (Chime::Unmuted, "unmuted"),
            (Chime::Deafened, "deafened"),
            (Chime::Undeafened, "undeafened"),
            (Chime::CameraOn, "cameraOn"),
            (Chime::CameraOff, "cameraOff"),
            (Chime::Disconnected, "disconnected"),
            (Chime::TalkStarted, "talkStarted"),
            (Chime::TalkStopped, "talkStopped"),
            (Chime::Moved, "moved"),
            (Chime::ViewerJoined, "viewerJoined"),
            (Chime::ViewerLeft, "viewerLeft"),
        ];

        for (chime, expected) in names {
            assert_eq!(serde_json::to_value(chime).unwrap(), expected);
        }
    }
}
