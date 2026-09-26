//! O que o receptor faz com pacote perdido: segura quem chegou adiantado, pede de novo o
//! que faltou (NACK), e quando a espera passa do prazo solta o buraco e pede um keyframe
//! (PLI).
//!
//! É o que o `rtpjitterbuffer` do GStreamer e o WebRTC do navegador fazem por baixo, e que
//! o caminho nativo não fazia: um pacote perdido quebrava a imagem até o próximo keyframe
//! periódico — dois segundos de tela congelada por um pacote. Aqui ele é pedido de novo, o
//! servidor o reenvia, e o quadro sai inteiro com um atraso de uma ida e volta.
//!
//! Tudo é lógica pura, com o relógio passado por quem chama: o receptor decide quando
//! perguntar, e os testes decidem que horas são.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Quanto se espera a retransmissão antes de pedir de novo. Uma ida e volta de internet
/// doméstica; em rede local a primeira volta muito antes.
const RETRY: Duration = Duration::from_millis(40);

/// Quantas vezes se pede o mesmo pacote.
const MOST_ASKS: u8 = 3;

/// Quanto um buraco pode segurar o fluxo antes de ser largado. Passou disso, esperar mais
/// custa mais que um keyframe.
const GIVE_UP: Duration = Duration::from_millis(250);

/// O máximo de pacotes segurados atrás de um buraco. Um quadro 1080p passa de cem pacotes;
/// mil é mais de meio segundo de tela a 20 Mb/s.
const MOST_HELD: usize = 1_024;

/// Um salto maior que isto não é perda, é outro fluxo — o producer recomeçou.
const JUMP: u64 = 3_000;

/// O que aconteceu com um fluxo até agora.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counters {
    /// Pacotes que chegaram, de primeira ou reenviados.
    pub received: u64,
    /// Pacotes que faltaram e chegaram depois de pedidos de novo.
    pub recovered: u64,
    /// Pacotes que nunca chegaram: o buraco foi largado.
    pub lost: u64,
}

/// O que fazer agora: pedir de novo, pedir keyframe, e soltar o que ficou pronto.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Due {
    pub nack: Vec<u16>,
    pub pli: bool,
    pub released: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy)]
struct Ask {
    since: Instant,
    asked: u8,
    last: Option<Instant>,
}

#[derive(Debug, Default)]
pub struct Recovery {
    /// O próximo número a soltar, já estendido além dos 16 bits do RTP.
    next: Option<u64>,
    held: BTreeMap<u64, Vec<u8>>,
    missing: BTreeMap<u64, Ask>,
    counters: Counters,
}

impl Recovery {
    /// Um pacote chegou. Devolve o que pode seguir agora, na ordem.
    pub fn arrive(&mut self, sequence: u16, packet: Vec<u8>, now: Instant) -> Vec<Vec<u8>> {
        self.counters.received += 1;

        let Some(next) = self.next else {
            self.next = Some(extended(sequence, None) + 1);

            return vec![packet];
        };

        let sequence = extended(sequence, Some(next));

        if sequence < next && next - sequence <= JUMP {
            // Atrasado demais: o buraco dele já foi largado, ou é um reenvio repetido.
            return Vec::new();
        }

        if sequence.abs_diff(next) > JUMP {
            self.held.clear();
            self.missing.clear();
            self.next = Some(sequence + 1);

            return vec![packet];
        }

        if self.missing.remove(&sequence).is_some() {
            self.counters.recovered += 1;
        }

        if sequence > next {
            for gap in next..sequence {
                if !self.held.contains_key(&gap) {
                    self.missing.entry(gap).or_insert(Ask { since: now, asked: 0, last: None });
                }
            }

            self.held.insert(sequence, packet);

            return Vec::new();
        }

        let mut released = vec![packet];

        self.next = Some(next + 1);
        self.release_ready(&mut released);

        released
    }

    /// O que o relógio pede: reenvios a pedir, e o buraco a largar quando passou do prazo.
    pub fn due(&mut self, now: Instant) -> Due {
        let mut due = Due::default();
        let stale = self
            .missing
            .first_key_value()
            .is_some_and(|(_, ask)| now.saturating_duration_since(ask.since) >= GIVE_UP);

        if (stale || self.held.len() > MOST_HELD)
            && let (Some(next), Some(&first_held)) = (self.next, self.held.keys().next())
        {
            self.counters.lost += first_held - next;
            self.missing.retain(|&sequence, _| sequence > first_held);
            self.next = Some(first_held);
            self.release_ready(&mut due.released);
            due.pli = true;
        }

        for (&sequence, ask) in &mut self.missing {
            let waited = ask.last.is_none_or(|last| now.saturating_duration_since(last) >= RETRY);

            if ask.asked < MOST_ASKS && waited {
                ask.asked += 1;
                ask.last = Some(now);
                #[allow(clippy::cast_possible_truncation)]
                due.nack.push(sequence as u16);
            }
        }

        due
    }

    pub fn counters(&self) -> Counters {
        self.counters
    }

    fn release_ready(&mut self, released: &mut Vec<Vec<u8>>) {
        while let Some(next) = self.next
            && let Some(packet) = self.held.remove(&next)
        {
            released.push(packet);
            self.missing.remove(&next);
            self.next = Some(next + 1);
        }
    }
}

/// O número de 16 bits do RTP estendido para 64, perto de `near`: é o que deixa o fluxo
/// passar da volta do 65535 para o 0 sem parecer que andou para trás. Começa longe do zero
/// para que "um antes" nunca vire subtração negativa.
fn extended(sequence: u16, near: Option<u64>) -> u64 {
    const CYCLE: u64 = 1 << 16;

    let Some(near) = near else {
        return (1 << 32) + u64::from(sequence);
    };
    let base = near & !(CYCLE - 1);

    [base - CYCLE, base, base + CYCLE]
        .into_iter()
        .map(|cycle| cycle + u64::from(sequence))
        .min_by_key(|candidate| candidate.abs_diff(near))
        .unwrap_or(base + u64::from(sequence))
}

/// O NACK genérico da RFC 4585 (PT 205, FMT 1), com os números agrupados de 17 em 17: o
/// primeiro de cada grupo vai inteiro e os 16 seguintes viram bits.
pub fn nack(sender: u32, media: u32, sequences: &[u16]) -> Vec<u8> {
    let mut sorted = sequences.to_vec();

    sorted.sort_unstable();
    sorted.dedup();

    let mut fields: Vec<(u16, u16)> = Vec::new();

    for sequence in sorted {
        match fields.last_mut() {
            Some((first, mask)) if sequence.wrapping_sub(*first) >= 1 && sequence.wrapping_sub(*first) <= 16 => {
                *mask |= 1 << (sequence.wrapping_sub(*first) - 1);
            }
            _ => fields.push((sequence, 0)),
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    let length = (2 + fields.len()) as u16;
    let mut packet = vec![0x81, 205];

    packet.extend(length.to_be_bytes());
    packet.extend(sender.to_be_bytes());
    packet.extend(media.to_be_bytes());

    for (first, mask) in fields {
        packet.extend(first.to_be_bytes());
        packet.extend(mask.to_be_bytes());
    }

    packet
}

/// O pedido de keyframe da RFC 4585 (PT 206, FMT 1).
pub fn pli(sender: u32, media: u32) -> Vec<u8> {
    let mut packet = vec![0x81, 206, 0, 2];

    packet.extend(sender.to_be_bytes());
    packet.extend(media.to_be_bytes());

    packet
}

/// Um pacote de retransmissão (RFC 4588) volta a ser o original: o número de sequência de
/// verdade está nos dois primeiros bytes do payload, e o SSRC e o tipo de payload são os do
/// fluxo principal. Pacote só de enchimento (o servidor os usa para medir a banda) não tem
/// original, e volta `None`.
pub fn unwrap_rtx(packet: &[u8], ssrc: u32, payload_type: u8) -> Option<Vec<u8>> {
    let header = header_length(packet)?;
    let padding = if packet[0] & 0x20 != 0 { usize::from(*packet.last()?) } else { 0 };
    let end = packet.len().checked_sub(padding)?;

    if end < header + 2 {
        return None;
    }

    let mut original = packet[..header].to_vec();

    original[0] &= !0x20;
    original[1] = (packet[1] & 0x80) | (payload_type & 0x7f);
    original[2..4].copy_from_slice(&packet[header..header + 2]);
    original[8..12].copy_from_slice(&ssrc.to_be_bytes());
    original.extend_from_slice(&packet[header + 2..end]);

    Some(original)
}

/// O tamanho do cabeçalho RTP: os 12 fixos, os CSRC e a extensão, quando há.
fn header_length(packet: &[u8]) -> Option<usize> {
    if packet.len() < 12 {
        return None;
    }

    let mut length = 12 + 4 * usize::from(packet[0] & 0x0f);

    if packet[0] & 0x10 != 0 {
        let words = u16::from_be_bytes([*packet.get(length + 2)?, *packet.get(length + 3)?]);

        length += 4 + 4 * usize::from(words);
    }

    (length <= packet.len()).then_some(length)
}

pub fn sequence_of(packet: &[u8]) -> Option<u16> {
    Some(u16::from_be_bytes([*packet.get(2)?, *packet.get(3)?]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(sequence: u16) -> Vec<u8> {
        let mut packet = vec![0x80, 96];

        packet.extend(sequence.to_be_bytes());
        packet.extend([0; 8]);
        packet.push(sequence as u8);

        packet
    }

    fn sequences(packets: &[Vec<u8>]) -> Vec<u16> {
        packets.iter().filter_map(|packet| sequence_of(packet)).collect()
    }

    #[test]
    fn packets_in_order_flow_straight_through() {
        let (mut recovery, now) = (Recovery::default(), Instant::now());

        for sequence in 10..15 {
            assert_eq!(sequences(&recovery.arrive(sequence, packet(sequence), now)), [sequence]);
        }

        assert_eq!(recovery.due(now), Due::default());
        assert_eq!(recovery.counters(), Counters { received: 5, recovered: 0, lost: 0 });
    }

    #[test]
    fn a_gap_holds_what_came_after_and_asks_for_what_is_missing() {
        let (mut recovery, now) = (Recovery::default(), Instant::now());

        recovery.arrive(1, packet(1), now);

        assert!(recovery.arrive(4, packet(4), now).is_empty(), "o 4 passou na frente do 2 e do 3");
        assert_eq!(recovery.due(now).nack, [2, 3]);
    }

    #[test]
    fn the_retransmission_fills_the_gap_and_everything_comes_out_in_order() {
        let (mut recovery, now) = (Recovery::default(), Instant::now());

        recovery.arrive(1, packet(1), now);
        recovery.arrive(4, packet(4), now);
        recovery.arrive(5, packet(5), now);

        assert!(recovery.arrive(3, packet(3), now).is_empty(), "o 2 ainda falta");
        assert_eq!(sequences(&recovery.arrive(2, packet(2), now)), [2, 3, 4, 5]);
        assert_eq!(recovery.counters().recovered, 2);
        assert_eq!(recovery.counters().lost, 0);
    }

    #[test]
    fn a_missing_packet_is_asked_again_after_the_retry_and_at_most_three_times() {
        let (mut recovery, now) = (Recovery::default(), Instant::now());

        recovery.arrive(1, packet(1), now);
        recovery.arrive(3, packet(3), now);

        assert_eq!(recovery.due(now).nack, [2]);
        assert!(recovery.due(now + Duration::from_millis(10)).nack.is_empty(), "pediu de novo cedo demais");
        assert_eq!(recovery.due(now + Duration::from_millis(40)).nack, [2]);
        assert_eq!(recovery.due(now + Duration::from_millis(80)).nack, [2]);
        assert!(recovery.due(now + Duration::from_millis(120)).nack.is_empty(), "pediu mais de três vezes");
    }

    #[test]
    fn a_hole_that_never_fills_is_given_up_with_a_keyframe_request() {
        let (mut recovery, now) = (Recovery::default(), Instant::now());

        recovery.arrive(1, packet(1), now);
        recovery.arrive(3, packet(3), now);
        recovery.arrive(4, packet(4), now);

        let due = recovery.due(now + Duration::from_millis(250));

        assert!(due.pli);
        assert_eq!(sequences(&due.released), [3, 4]);
        assert_eq!(recovery.counters().lost, 1);
        assert_eq!(sequences(&recovery.arrive(5, packet(5), now)), [5], "depois de largar, o fluxo segue");
        assert!(recovery.arrive(2, packet(2), now).is_empty(), "o largado que chega tarde não volta");
    }

    #[test]
    fn the_sequence_wraps_from_65535_to_0_without_looking_like_a_gap() {
        let (mut recovery, now) = (Recovery::default(), Instant::now());

        for sequence in [65_534, 65_535, 0, 1] {
            assert_eq!(sequences(&recovery.arrive(sequence, packet(sequence), now)), [sequence]);
        }

        assert!(recovery.due(now).nack.is_empty());
    }

    #[test]
    fn a_huge_jump_is_a_new_stream_and_not_thousands_of_losses() {
        let (mut recovery, now) = (Recovery::default(), Instant::now());

        recovery.arrive(100, packet(100), now);

        assert_eq!(sequences(&recovery.arrive(20_000, packet(20_000), now)), [20_000]);
        assert!(recovery.due(now).nack.is_empty());
        assert_eq!(recovery.counters().lost, 0);
    }

    /// O producer que recomeça com um número menor não pode virar "atrasado para sempre":
    /// seria o fluxo inteiro jogado fora, pacote por pacote.
    #[test]
    fn a_huge_jump_backwards_is_a_new_stream_too() {
        let (mut recovery, now) = (Recovery::default(), Instant::now());

        recovery.arrive(30_000, packet(30_000), now);

        assert_eq!(sequences(&recovery.arrive(10_000, packet(10_000), now)), [10_000]);
        assert_eq!(sequences(&recovery.arrive(10_001, packet(10_001), now)), [10_001]);
    }

    #[test]
    fn the_nack_groups_seventeen_numbers_per_field() {
        let built = nack(0xAAAA_AAAA, 0xBBBB_BBBB, &[100, 101, 103, 116, 117]);

        assert_eq!(&built[..2], &[0x81, 205]);
        // Dois campos: 100 com os bits de 101, 103 e 116; e 117 sozinho.
        assert_eq!(u16::from_be_bytes([built[2], built[3]]), 4);
        assert_eq!(&built[12..16], &[0, 100, 0x80, 0x05]);
        assert_eq!(&built[16..20], &[0, 117, 0, 0]);
    }

    #[test]
    fn the_keyframe_request_names_the_stream() {
        let built = pli(1, 0x0102_0304);

        assert_eq!(built, [0x81, 206, 0, 2, 0, 0, 0, 1, 1, 2, 3, 4]);
    }

    #[test]
    fn a_retransmission_becomes_the_original_packet() {
        // RTX: SSRC 0x99, payload 97, sequência própria 7, e o número original 1234 na frente.
        let mut rtx = vec![0x80, 0x80 | 97, 0, 7, 0, 0, 0, 9, 0, 0, 0, 0x99];

        rtx.extend(1234_u16.to_be_bytes());
        rtx.extend([0xDE, 0xAD]);

        let original = unwrap_rtx(&rtx, 0x55, 96).expect("tem original");

        assert_eq!(sequence_of(&original), Some(1234));
        assert_eq!(original[1], 0x80 | 96, "o marcador fica, o tipo volta ao original");
        assert_eq!(&original[8..12], &[0, 0, 0, 0x55]);
        assert_eq!(&original[12..], &[0xDE, 0xAD]);
    }

    #[test]
    fn a_padding_only_retransmission_has_no_original() {
        let mut probe = vec![0xA0, 97, 0, 8, 0, 0, 0, 9, 0, 0, 0, 0x99];

        probe.extend([0; 3]);
        probe.push(4);

        assert!(unwrap_rtx(&probe, 0x55, 96).is_none());
    }
}
