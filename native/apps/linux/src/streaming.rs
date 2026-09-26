//! O que a sala anuncia, no formato que as telas do Linux desenham.
//!
//! Publicar, consumir, recuperar pacote e reabrir depois de uma queda são do
//! `core_app::room::Room` — o mesmo do macOS e do Windows. Aqui só mora a leitura dos avisos
//! dele (`room.tiles`, `room.mine`) para os tipos que as telas já conhecem.

use core_app::models::Peer;
use serde::Deserialize;
use serde_json::Value;

/// O que esta pessoa está mandando, e o que ela tem permissão de mandar. A interface só
/// esconde botão — quem autoriza é o `can` do servidor.
pub use core_app::room::Mine;

/// Uma transmissão sendo assistida, do jeito que a janela desenha.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    pub producer_id: String,
    pub label: String,
    /// Câmera é cartão pequeno; tela é o palco.
    #[serde(default)]
    pub camera: bool,
}

/// O `room.tiles`: as telas que estão sendo assistidas agora.
pub fn tiles_of(data: &Value) -> Vec<Tile> {
    serde_json::from_value(data["tiles"].clone()).unwrap_or_default()
}

/// O `room.mine`.
pub fn mine_of(data: &Value) -> Mine {
    serde_json::from_value(data.clone()).unwrap_or_default()
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
    fn the_announced_tiles_become_the_cards_the_window_draws() {
        let tiles = tiles_of(&json!({
            "tiles": [
                { "producerId": "tela", "peerId": "p", "label": "Ada", "camera": false, "mine": false, "paused": false, "audio": null },
                { "producerId": "cam", "peerId": "q", "label": "Bia", "camera": true },
            ],
            "pending": [],
        }));

        assert_eq!(tiles.len(), 2);
        assert_eq!((tiles[0].producer_id.as_str(), tiles[0].label.as_str(), tiles[0].camera), ("tela", "Ada", false));
        assert!(tiles[1].camera);
    }

    #[test]
    fn the_announced_peers_keep_who_i_am() {
        let peers = peers_of(&json!({ "peers": [
            { "peerId": "a", "name": "Ada", "producers": [], "selfPeer": true },
            { "peerId": "b", "name": "Bia", "producers": [], "selfPeer": false },
        ] }));

        assert!(peers[0].self_peer && !peers[1].self_peer);
    }

    #[test]
    fn a_broken_announcement_draws_nothing_instead_of_crashing() {
        assert!(tiles_of(&json!({ "tiles": "?" })).is_empty());
        assert_eq!(mine_of(&json!(null)), Mine::default());
    }
}
