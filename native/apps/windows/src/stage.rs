//! O que a sala anunciou, guardado do jeito que a janela desenha: os cartões do palco, quem
//! está dentro, e o microfone.
//!
//! Tudo aqui é dado puro — a ponte o alimenta com os avisos do `Room` e pinta o resultado.
//! A grade, o foco e a tela cheia são só do lado de quem olha, como no React e no Mac.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use core_app::models::Peer;
use core_app::permissions::MemberActions;
use core_app::room::Mine;
use serde::Deserialize;
use serde_json::Value;

/// Um cartão, como o `room.tiles` o descreve.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    pub producer_id: String,
    pub label: String,
    #[serde(default)]
    pub camera: bool,
    #[serde(default)]
    pub mine: bool,
    #[serde(default)]
    pub paused: bool,
    /// O producer do som que acompanha esta tela.
    #[serde(default)]
    pub audio: Option<String>,
}

/// Onde um cartão cai na chamada. A grade (colunas = ⌈√n⌉) é conta da própria tela, que
/// também sabe quantas pessoas sem vídeo entram nela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub tile: Tile,
    /// A posição entre os que não estão no foco.
    pub rank: usize,
    pub focused: bool,
    pub full: bool,
    pub heard: bool,
    /// De 0 a 100, só deste lado.
    pub volume: u8,
    pub watchers: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Stage {
    tiles: Vec<Tile>,
    pending: usize,
    watchers: HashMap<String, Vec<String>>,
    /// As telas cujo som a pessoa ligou, pelo producer do vídeo.
    heard: HashSet<String>,
    /// O volume escolhido de cada tela, de 0 a 100; sem escolha, 100.
    volumes: HashMap<String, u8>,
    focused: Option<String>,
    full: Option<String>,
}

impl Stage {
    /// O `room.tiles`. Foco e tela cheia de um cartão que sumiu somem junto.
    pub fn set_tiles(&mut self, data: &Value) {
        self.tiles = serde_json::from_value(data["tiles"].clone()).unwrap_or_default();
        self.pending = data["pending"].as_array().map_or(0, Vec::len);

        let alive: HashSet<&str> = self.tiles.iter().map(|tile| tile.producer_id.as_str()).collect();

        self.heard.retain(|producer| alive.contains(producer.as_str()));
        self.volumes.retain(|producer, _| alive.contains(producer.as_str()));
        self.focused.take_if(|producer| !alive.contains(producer.as_str()));
        self.full.take_if(|producer| !alive.contains(producer.as_str()));
    }

    /// O `room.watchers`: quem está assistindo a uma tela.
    pub fn set_watchers(&mut self, data: &Value) {
        let Some(producer) = data["producerId"].as_str() else {
            return;
        };
        let names = data["watchers"]
            .as_array()
            .map(|watchers| watchers.iter().filter_map(|watcher| watcher["name"].as_str().map(str::to_owned)).collect())
            .unwrap_or_default();

        self.watchers.insert(producer.to_owned(), names);
    }

    pub fn pending(&self) -> usize {
        self.pending
    }

    pub fn tile(&self, producer: &str) -> Option<&Tile> {
        self.tiles.iter().find(|tile| tile.producer_id == producer)
    }

    /// Liga ou desliga o som de uma tela. Devolve o producer do som e se ele passou a tocar.
    /// Ligar com o volume em zero volta a 100, como no React: ligado e mudo ao mesmo tempo não
    /// diria nada a ninguém.
    pub fn toggle_heard(&mut self, producer: &str) -> Option<(String, bool)> {
        let audio = self.tile(producer)?.audio.clone()?;
        let heard = !self.heard.remove(producer);

        if heard {
            self.heard.insert(producer.to_owned());

            if self.volume(producer) == 0 {
                self.volumes.insert(producer.to_owned(), 100);
            }
        }

        Some((audio, heard))
    }

    /// O volume do slider, de 0 a 100. Como no React, mexer nele liga o som e zero o desliga —
    /// o som de uma tela chega mudo, e arrastar o volume de uma tela muda não mudaria nada.
    /// Devolve o producer do som e se ele passou a tocar, quando isso mudou.
    pub fn set_volume(&mut self, producer: &str, volume: u8) -> Option<(String, Option<bool>)> {
        let audio = self.tile(producer)?.audio.clone()?;
        let volume = volume.min(100);
        let heard = volume > 0;
        let changed = heard != self.heard.contains(producer);

        self.volumes.insert(producer.to_owned(), volume);

        if heard {
            self.heard.insert(producer.to_owned());
        } else {
            self.heard.remove(producer);
        }

        Some((audio, changed.then_some(heard)))
    }

    pub fn volume(&self, producer: &str) -> u8 {
        self.volumes.get(producer).copied().unwrap_or(100)
    }

    /// Focar o mesmo cartão de novo, ou pedir foco em nada, sai do foco.
    pub fn toggle_focus(&mut self, producer: &str) {
        self.focused = (self.focused.as_deref() != Some(producer) && !producer.is_empty()).then(|| producer.to_owned());
    }

    pub fn toggle_full(&mut self, producer: &str) {
        self.full = (self.full.as_deref() != Some(producer) && !producer.is_empty()).then(|| producer.to_owned());
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Há um cartão no foco e outros para ir à fila de baixo.
    pub fn focusing(&self) -> bool {
        self.focused.is_some() && self.tiles.len() > 1 && self.full.is_none()
    }

    pub fn full_screen(&self) -> bool {
        self.full.is_some()
    }

    /// A transmissão em tela cheia, se há uma.
    pub fn full_producer(&self) -> Option<String> {
        self.full.clone()
    }

    pub fn placed(&self) -> Vec<Placed> {
        let mut rank = 0;

        self.tiles
            .iter()
            .map(|tile| {
                let focused = self.focused.as_deref() == Some(tile.producer_id.as_str());
                let placed = Placed {
                    rank,
                    focused,
                    full: self.full.as_deref() == Some(tile.producer_id.as_str()),
                    heard: self.heard.contains(&tile.producer_id),
                    volume: self.volume(&tile.producer_id),
                    watchers: self.watchers.get(&tile.producer_id).cloned().unwrap_or_default(),
                    tile: tile.clone(),
                };

                if !focused {
                    rank += 1;
                }

                placed
            })
            .collect()
    }
}

/// O microfone e o som de quem está na voz, com as contas que a barra de baixo desenha.
#[derive(Debug, Default)]
pub struct Voice {
    pub mine: Mine,
    /// Dentro de uma sala com voz. A sala por código não tem microfone.
    pub inside: bool,
    /// Do clique no canal até o microfone abrir.
    pub opening: bool,
    /// O mudo de quem está fora da sala. Ao sair, o mudo de dentro vira este.
    pub muted_at_rest: bool,
    pub deafened: bool,
    /// A última vez que o próprio microfone passou do limiar de fala.
    pub spoke_at: Option<Instant>,
    /// Os producers de microfone de quem está falando agora.
    pub speaking: HashSet<String>,
    /// Quem está na sala, como o último `room.peers` contou.
    pub peers: Vec<Peer>,
    /// O que eu posso com cada membro do servidor aberto (pelo id de usuário), e quem está
    /// mutado pelo servidor — vem da árvore, e a lista da voz desenha por aqui.
    pub moderation: HashMap<i64, MemberActions>,
    pub server_muted: HashSet<i64>,
    /// Quem eu calei só para mim, pelo menu.
    pub muted_people: HashSet<i64>,
    /// "Silenciar ao entrar": o microfone nasce mutado em cada voz.
    pub mute_on_join: bool,
}

impl Voice {
    pub fn mic_shown_off(&self) -> bool {
        if self.inside {
            self.mine.mic_shown_off(self.opening)
        } else {
            self.muted_at_rest
        }
    }

    /// Um `room.level` do próprio microfone, na escala de 0 a 100.
    pub fn hear_myself(&mut self, percent: u8, now: Instant) {
        if percent >= core_app::speaking::OWN_LOUDNESS {
            self.spoke_at = Some(now);
        }
    }

    /// Eu falando: na voz, com o microfone não desenhado como mudo, e acima do limiar há
    /// menos que a cauda — a mesma dos outros, para o anel não piscar entre as palavras.
    pub fn speaking_myself(&self) -> bool {
        self.inside
            && !self.mic_shown_off()
            && self.spoke_at.is_some_and(|spoke| spoke.elapsed() < core_app::speaking::TAIL)
    }

    /// A pessoa saiu da sala: o mudo de dentro vira o de fora, e o resto zera.
    pub fn leave(&mut self) {
        if self.inside {
            self.muted_at_rest = self.mine.mic && self.mine.mic_muted;
        }

        self.mine = Mine::default();
        self.inside = false;
        self.opening = false;
        self.spoke_at = None;
        self.speaking.clear();
        self.peers.clear();
        self.muted_people.clear();
    }

    /// O id de usuário de quem está na sala; visitante da sala por código não tem.
    pub fn user_of(peer: &Peer) -> Option<i64> {
        peer.user_id.as_deref()?.strip_prefix("user:")?.parse().ok()
    }

    /// O producer do microfone de uma pessoa, pelo id de usuário.
    pub fn microphone_of(&self, user: i64) -> Option<String> {
        self.peers
            .iter()
            .find(|peer| Self::user_of(peer) == Some(user))?
            .producers
            .iter()
            .find(|producer| producer.source == "mic")
            .map(|producer| producer.producer_id.clone())
    }

    /// Alguém dentro da sala está falando: pelo microfone dele, ou por mim mesmo.
    pub fn is_speaking(&self, peer: &Peer) -> bool {
        if peer.self_peer {
            return self.speaking_myself();
        }

        peer.producers
            .iter()
            .any(|producer| producer.source == "mic" && self.speaking.contains(&producer.producer_id))
    }
}

/// A linha de números de uma tela, como o React a escreve: `1080p · 60 fps · 0,4%`. A
/// perda é a do último segundo — a acumulada esconderia a rede que apertou agora.
pub fn stats_line(frames: u32, height: u32, received: u64, lost: u64) -> (String, bool) {
    let total = received + lost;
    #[allow(clippy::cast_precision_loss)]
    let loss = if total == 0 { 0.0 } else { lost as f64 * 100.0 / total as f64 };
    let resolution = if height == 0 { "—".to_owned() } else { format!("{height}p") };
    let said = format!("{resolution} · {frames} fps · {}", format!("{loss:.1}%").replace('.', ","));

    (said, loss >= 2.0)
}

/// O `room.peers`. O `selfPeer` é lido à parte: no modelo ele não vem do servidor (é o
/// núcleo que o marca), e a leitura direta o deixaria sempre falso.
pub fn peers_of(data: &Value) -> Vec<Peer> {
    data["peers"]
        .as_array()
        .map(|peers| {
            peers
                .iter()
                .filter_map(|value| {
                    let mut peer: Peer = serde_json::from_value(value.clone()).ok()?;

                    peer.self_peer = value["selfPeer"].as_bool().unwrap_or(false);

                    Some(peer)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_stats_line_reads_like_the_react_one() {
        assert_eq!(stats_line(60, 1_080, 900, 0), ("1080p · 60 fps · 0,0%".to_owned(), false));
        assert_eq!(stats_line(58, 720, 97, 3), ("720p · 58 fps · 3,0%".to_owned(), true));
        assert_eq!(stats_line(0, 0, 0, 0), ("— · 0 fps · 0,0%".to_owned(), false));
    }

    #[test]
    fn the_announced_peers_keep_who_i_am() {
        let peers = peers_of(&json!({ "peers": [
            { "peerId": "a", "name": "Ada", "producers": [], "reconnecting": false, "selfPeer": true, "userId": null },
            { "peerId": "b", "name": "Bia", "producers": [{ "producerId": "m", "kind": "audio", "source": "mic", "paused": false }], "selfPeer": false },
        ] }));

        assert_eq!(peers.len(), 2);
        assert!(peers[0].self_peer && !peers[1].self_peer);
        assert_eq!(peers[1].producers[0].source, "mic");
    }

    #[test]
    fn someone_speaks_through_their_own_microphone() {
        let peers = peers_of(&json!({ "peers": [
            { "peerId": "b", "name": "Bia", "producers": [{ "producerId": "m", "kind": "audio", "source": "mic" }] },
        ] }));
        let mut voice = Voice::default();

        assert!(!voice.is_speaking(&peers[0]));

        voice.speaking.insert("m".to_owned());

        assert!(voice.is_speaking(&peers[0]));
    }

    fn tiles(count: usize) -> Value {
        let tiles: Vec<Value> = (0..count)
            .map(|index| json!({ "producerId": format!("tela-{index}"), "peerId": "p", "label": format!("Pessoa {index}"), "camera": false, "mine": false, "paused": false, "audio": format!("som-{index}") }))
            .collect();

        json!({ "tiles": tiles, "pending": [] })
    }

    #[test]
    fn a_person_in_the_room_is_known_by_the_account_id_and_the_microphone() {
        let peers = peers_of(&json!({ "peers": [
            { "peerId": "b", "name": "Bia", "userId": "user:12", "producers": [{ "producerId": "m", "kind": "audio", "source": "mic" }] },
            { "peerId": "g", "name": "Visita", "userId": "guest:abc", "producers": [] },
        ] }));
        let voice = Voice { peers, ..Voice::default() };

        assert_eq!(Voice::user_of(&voice.peers[0]), Some(12));
        assert_eq!(Voice::user_of(&voice.peers[1]), None);
        assert_eq!(voice.microphone_of(12).as_deref(), Some("m"));
        assert_eq!(voice.microphone_of(99), None);
    }

    #[test]
    fn focusing_puts_one_on_stage_and_ranks_the_others_below() {
        let mut stage = Stage::default();

        stage.set_tiles(&tiles(3));
        stage.toggle_focus("tela-1");

        let placed = stage.placed();

        assert!(stage.focusing());
        assert!(placed[1].focused);
        assert_eq!((placed[0].rank, placed[2].rank), (0, 1));

        stage.toggle_focus("tela-1");

        assert!(!stage.focusing());
    }

    #[test]
    fn a_single_tile_in_focus_is_not_a_focus_layout() {
        let mut stage = Stage::default();

        stage.set_tiles(&tiles(1));
        stage.toggle_focus("tela-0");

        assert!(!stage.focusing());
    }

    #[test]
    fn focus_fullscreen_and_sound_of_a_tile_that_left_are_forgotten() {
        let mut stage = Stage::default();

        stage.set_tiles(&tiles(2));
        stage.toggle_focus("tela-1");
        stage.toggle_full("tela-1");
        stage.toggle_heard("tela-1");
        stage.set_tiles(&tiles(1));

        assert!(!stage.focusing() && !stage.full_screen());
        assert!(!stage.placed()[0].heard);
    }

    #[test]
    fn turning_a_screen_sound_on_names_its_audio_producer() {
        let mut stage = Stage::default();

        stage.set_tiles(&tiles(1));

        assert_eq!(stage.toggle_heard("tela-0"), Some(("som-0".to_owned(), true)));
        assert_eq!(stage.toggle_heard("tela-0"), Some(("som-0".to_owned(), false)));
        assert_eq!(stage.toggle_heard("nenhuma"), None);
    }

    #[test]
    fn the_volume_slider_turns_the_sound_on_and_zero_mutes_it() {
        let mut stage = Stage::default();

        stage.set_tiles(&tiles(1));

        assert_eq!(stage.set_volume("tela-0", 40), Some(("som-0".to_owned(), Some(true))), "arrastar liga o som");
        assert_eq!(stage.set_volume("tela-0", 70), Some(("som-0".to_owned(), None)), "já tocando, só muda o volume");
        assert_eq!(stage.placed()[0].volume, 70);
        assert_eq!(stage.set_volume("tela-0", 0), Some(("som-0".to_owned(), Some(false))), "zero muta");
        assert!(!stage.placed()[0].heard);

        stage.toggle_heard("tela-0");

        assert_eq!(stage.placed()[0].volume, 100, "ligar com o volume em zero volta a 100");
        assert!(stage.placed()[0].heard);
    }

    #[test]
    fn the_watchers_are_counted_by_screen() {
        let mut stage = Stage::default();

        stage.set_tiles(&tiles(1));
        stage.set_watchers(&json!({ "producerId": "tela-0", "watchers": [{ "name": "Ada", "peerId": "a" }, { "name": "Bia", "peerId": "b" }] }));

        assert_eq!(stage.placed()[0].watchers, ["Ada", "Bia"]);
    }

    #[test]
    fn the_pending_count_is_what_is_live_but_not_open() {
        let mut stage = Stage::default();

        stage.set_tiles(&json!({ "tiles": [], "pending": [{ "producerId": "x" }, { "producerId": "y" }] }));

        assert_eq!(stage.pending(), 2);
    }

    #[test]
    fn outside_a_room_the_mic_is_the_mute_at_rest() {
        let voice = Voice { muted_at_rest: true, ..Voice::default() };

        assert!(voice.mic_shown_off());
        assert!(!Voice::default().mic_shown_off());
    }

    #[test]
    fn inside_while_opening_the_mic_does_not_blink_muted() {
        let voice = Voice {
            inside: true,
            opening: true,
            mine: Mine { can_speak: true, ..Mine::default() },
            ..Voice::default()
        };

        assert!(!voice.mic_shown_off());
    }

    #[test]
    fn leaving_muted_keeps_the_mute_outside() {
        let mut voice = Voice {
            inside: true,
            mine: Mine { mic: true, mic_muted: true, can_speak: true, ..Mine::default() },
            ..Voice::default()
        };

        voice.leave();

        assert!(voice.muted_at_rest && !voice.inside);
        assert!(voice.mic_shown_off());
    }

    #[test]
    fn leaving_when_no_room_was_open_keeps_the_mute_at_rest() {
        let mut voice = Voice { muted_at_rest: true, ..Voice::default() };

        voice.leave();

        assert!(voice.muted_at_rest);
    }

    #[test]
    fn i_speak_only_inside_with_the_mic_on_and_above_the_threshold() {
        let mut voice = Voice {
            inside: true,
            mine: Mine { mic: true, can_speak: true, ..Mine::default() },
            ..Voice::default()
        };

        voice.hear_myself(core_app::speaking::OWN_LOUDNESS - 1, Instant::now());

        assert!(!voice.speaking_myself(), "abaixo do limiar não acende");

        voice.hear_myself(core_app::speaking::OWN_LOUDNESS, Instant::now());

        assert!(voice.speaking_myself());

        voice.mine.mic_muted = true;

        assert!(!voice.speaking_myself());
    }

    #[test]
    fn my_ring_stays_lit_through_the_tail_and_goes_out_after_it() {
        let mut voice = Voice {
            inside: true,
            mine: Mine { mic: true, can_speak: true, ..Mine::default() },
            ..Voice::default()
        };
        let long_ago = Instant::now().checked_sub(core_app::speaking::TAIL).expect("relógio");

        voice.hear_myself(100, long_ago);
        voice.hear_myself(0, Instant::now());

        assert!(!voice.speaking_myself(), "o silêncio depois da cauda apaga");

        voice.hear_myself(100, Instant::now());
        voice.hear_myself(0, Instant::now());

        assert!(voice.speaking_myself(), "o silêncio logo depois da fala ainda não apaga");
    }
}
