//! O que a sala anunciou, guardado do jeito que a janela desenha: os cartões do palco, quem
//! está dentro, e o microfone.
//!
//! Tudo aqui é dado puro — a ponte o alimenta com os avisos do `Room` e pinta o resultado.
//! A grade, o foco e a tela cheia são só do lado de quem olha, como no React e no Mac.

use std::collections::{HashMap, HashSet};

use core_app::models::Peer;
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

/// Onde um cartão cai no palco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub tile: Tile,
    pub column: usize,
    pub line: usize,
    /// A posição entre os que não estão no foco.
    pub rank: usize,
    pub focused: bool,
    pub full: bool,
    pub heard: bool,
    pub watchers: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Stage {
    tiles: Vec<Tile>,
    pending: usize,
    watchers: HashMap<String, Vec<String>>,
    /// As telas cujo som a pessoa ligou, pelo producer do vídeo.
    heard: HashSet<String>,
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
    pub fn toggle_heard(&mut self, producer: &str) -> Option<(String, bool)> {
        let audio = self.tile(producer)?.audio.clone()?;
        let heard = !self.heard.remove(producer);

        if heard {
            self.heard.insert(producer.to_owned());
        }

        Some((audio, heard))
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

    /// Colunas e linhas da grade: uma com uma tela, duas até quatro, três daí em diante.
    pub fn grid(&self) -> (usize, usize) {
        let count = self.tiles.len().max(1);
        let columns = match count {
            1 => 1,
            2..=4 => 2,
            _ => 3,
        };

        (columns, count.div_ceil(columns))
    }

    /// Há um cartão no foco e outros para ir à fila de baixo.
    pub fn focusing(&self) -> bool {
        self.focused.is_some() && self.tiles.len() > 1 && self.full.is_none()
    }

    pub fn full_screen(&self) -> bool {
        self.full.is_some()
    }

    pub fn placed(&self) -> Vec<Placed> {
        let (columns, _) = self.grid();
        let mut rank = 0;

        self.tiles
            .iter()
            .enumerate()
            .map(|(index, tile)| {
                let focused = self.focused.as_deref() == Some(tile.producer_id.as_str());
                let placed = Placed {
                    column: index % columns,
                    line: index / columns,
                    rank,
                    focused,
                    full: self.full.as_deref() == Some(tile.producer_id.as_str()),
                    heard: self.heard.contains(&tile.producer_id),
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
    /// O nível do próprio microfone, de 0 a 1.
    pub level: f32,
    /// Os producers de microfone de quem está falando agora.
    pub speaking: HashSet<String>,
    /// Quem está na sala, como o último `room.peers` contou.
    pub peers: Vec<Peer>,
}

impl Voice {
    pub fn mic_shown_off(&self) -> bool {
        if self.inside {
            self.mine.mic_shown_off(self.opening)
        } else {
            self.muted_at_rest
        }
    }

    /// Eu falando: na voz, com o microfone não desenhado como mudo e acima do limiar.
    pub fn speaking_myself(&self) -> bool {
        self.inside && !self.mic_shown_off() && self.level > core_app::speaking::LOUDNESS
    }

    /// A pessoa saiu da sala: o mudo de dentro vira o de fora, e o resto zera.
    pub fn leave(&mut self) {
        if self.inside {
            self.muted_at_rest = self.mine.mic && self.mine.mic_muted;
        }

        self.mine = Mine::default();
        self.inside = false;
        self.opening = false;
        self.level = 0.0;
        self.speaking.clear();
        self.peers.clear();
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
    fn the_grid_has_one_column_for_one_two_up_to_four_and_three_after() {
        let mut stage = Stage::default();

        for (count, expected) in [(1, (1, 1)), (2, (2, 1)), (3, (2, 2)), (4, (2, 2)), (5, (3, 2)), (7, (3, 3))] {
            stage.set_tiles(&tiles(count));

            assert_eq!(stage.grid(), expected, "{count} telas");
        }
    }

    #[test]
    fn each_tile_falls_in_its_column_and_line() {
        let mut stage = Stage::default();

        stage.set_tiles(&tiles(5));

        let places: Vec<(usize, usize)> = stage.placed().iter().map(|placed| (placed.column, placed.line)).collect();

        assert_eq!(places, [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1)]);
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
            level: 0.3,
            ..Voice::default()
        };

        assert!(voice.speaking_myself());

        voice.mine.mic_muted = true;

        assert!(!voice.speaking_myself());
    }
}
