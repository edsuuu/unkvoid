//! A taxa do vídeo seguindo a perda do caminho.
//!
//! Taxa fixa com o uplink saturado é empurrar mais do que passa: o que não cabe vira perda,
//! a perda vira reenvio, e o reenvio disputa o mesmo uplink. Aqui a taxa desce quando a
//! perda diz que é congestionamento e volta devagar quando ela some.
//!
//! O sinal é a **perda**, e não a estimativa de banda do servidor: o mediasoup só estima
//! com `abs-send-time`, e estimar por atraso sem um pacer no remetente acusaria
//! congestionamento a cada quadro-chave. Nada aqui lê rede nem relógio: quem chama entrega
//! os números de uma janela (~1 s) e recebe a decisão.

/// Com menos pacotes do que isto a porcentagem é ruído: três perdidos em quarenta dariam
/// 7% numa tela parada. O que a janela trouxe fica guardado e soma com a seguinte, em vez
/// de ser jogado fora — senão uma transmissão de 720p15 já no piso (~60 pacotes por
/// segundo) nunca mais juntaria amostra para subir de volta.
const MIN_PACKETS: u64 = 100;

/// Perda a partir da qual a janela é congestionamento, em ‰. Perda leve e aleatória (Wi‑Fi,
/// a rota longa do Brasil aos EUA) fica em 1 a 2% e o reenvio por NACK dá conta dela;
/// baixar a taxa por causa dela pioraria a imagem sem ganhar um pacote.
const CONGESTED_PERMILLE: u64 = 50;

/// Abaixo disto a janela conta como limpa, em ‰.
const CLEAN_PERMILLE: u64 = 10;

/// Janelas limpas seguidas antes de cada subida. Descer é rápido e subir é devagar de
/// propósito: subir cedo demais devolve o congestionamento que acabou de passar. As
/// janelas da carência não entram na conta: limpas ou não, elas ainda medem a taxa antiga.
const CLEAN_WINDOWS: u32 = 5;

/// Quantas janelas medidas depois de uma queda o governador fica sem poder cair de novo.
/// O MFT da NVIDIA leva de 6 a 8 s para chegar à taxa nova — medido na RTX 4060 Ti, com o
/// exemplo `encoder` — e nesse meio-tempo a perda que se vê ainda é a da taxa antiga:
/// decidir antes disso é punir duas vezes a mesma perda, e era o que levava qualquer
/// congestionamento direto ao piso. A perda continua sendo medida e publicada; só não decide.
const HOLDOFF_WINDOWS: u32 = 8;

/// Com a carência cada queda é uma decisão rara, então o degrau é grande: de 10 Mb/s vai a
/// 7, a 4,9 e ao piso em três quedas (~19 s de congestionamento sustentado). Com 15% por
/// queda seriam sete, e quase um minuto empurrando mais do que o caminho leva.
const DECREASE_PERCENT: u64 = 70;
const INCREASE_PERCENT: u64 = 105;

/// O piso é o que limita o estrago se a heurística errar: a taxa nunca cai a ponto de
/// destruir a imagem, por pior que seja a perda medida.
const FLOOR_PERCENT: u64 = 35;

pub struct BitrateGovernor {
    ceiling: u32,
    floor: u32,
    target: u32,
    enabled: bool,
    clean_windows: u32,
    holdoff: u32,
    sent: u64,
    nacked: u64,
    dropped: u64,
    loss_permille: u32,
}

impl BitrateGovernor {
    /// `ceiling` é a taxa da qualidade escolhida, e é de onde a transmissão parte.
    pub fn new(ceiling: u32, enabled: bool) -> Self {
        Self {
            ceiling,
            floor: scale(ceiling, FLOOR_PERCENT),
            target: ceiling,
            enabled,
            clean_windows: 0,
            holdoff: 0,
            sent: 0,
            nacked: 0,
            dropped: 0,
            loss_permille: 0,
        }
    }

    /// `UNKVOID_ABR=off` deixa a taxa fixa no teto. Os limiares acima saíram de conta, não
    /// de medição em campo, e rede de verdade precisa de um jeito de tirar isto da frente
    /// para comparar. A perda continua sendo medida, que é o que se quer ver nessa hora.
    pub fn from_env(ceiling: u32) -> Self {
        let off = std::env::var("UNKVOID_ABR").is_ok_and(|value| value == "off");

        Self::new(ceiling, !off)
    }

    pub fn target(&self) -> u32 {
        self.target
    }

    /// A perda da última janela que teve pacotes o bastante para medir, em ‰.
    pub fn loss_permille(&self) -> u32 {
        self.loss_permille
    }

    /// O encoder recusou a troca: a taxa volta a ser a fixa, e ninguém tenta de novo.
    pub fn give_up(&mut self) {
        self.enabled = false;
        self.target = self.ceiling;
    }

    /// Uma janela: pacotes de vídeo que saíram, os que o servidor pediu de volta (cada um
    /// contado uma vez) e os que o buffer de saída cheio largou. Devolve a taxa nova
    /// quando ela muda.
    pub fn observe(&mut self, sent: u64, nacked: u64, dropped: u64) -> Option<u32> {
        self.sent += sent;
        self.nacked += nacked;
        self.dropped += dropped;

        if self.sent < MIN_PACKETS {
            return None;
        }

        // O maior dos dois, e não a soma: o pacote que o buffer largou nunca chegou ao
        // servidor, então ele também vem pedido de volta. Somar contaria a mesma perda
        // duas vezes; olhar só o NACK ficaria cego quando o caminho de volta não abre.
        let lost = self.nacked.max(self.dropped);
        let loss = (lost * 1000 / self.sent).min(1000);

        self.sent = 0;
        self.nacked = 0;
        self.dropped = 0;
        self.loss_permille = loss as u32;

        if !self.enabled {
            return None;
        }

        if self.holdoff > 0 {
            self.holdoff -= 1;

            return None;
        }

        let wanted = if loss >= CONGESTED_PERMILLE {
            self.clean_windows = 0;

            scale(self.target, DECREASE_PERCENT).max(self.floor)
        } else if loss < CLEAN_PERMILLE {
            self.clean_windows += 1;

            if self.clean_windows < CLEAN_WINDOWS {
                return None;
            }

            self.clean_windows = 0;

            scale(self.target, INCREASE_PERCENT).min(self.ceiling)
        } else {
            self.clean_windows = 0;

            return None;
        };

        if wanted == self.target {
            return None;
        }

        if wanted < self.target {
            self.holdoff = HOLDOFF_WINDOWS;
        }

        self.target = wanted;

        Some(wanted)
    }
}

fn scale(bitrate: u32, percent: u64) -> u32 {
    (u64::from(bitrate) * percent / 100) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const CEILING: u32 = 10_000_000;

    /// Uma janela de 1080p60: uns mil pacotes por segundo.
    fn window(governor: &mut BitrateGovernor, loss_permille: u64) -> Option<u32> {
        governor.observe(1000, loss_permille, 0)
    }

    #[test]
    fn congestion_lowers_the_target() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        assert_eq!(window(&mut governor, 50), Some(7_000_000));
        assert_eq!(governor.target(), 7_000_000);
        assert_eq!(governor.loss_permille(), 50);
    }

    #[test]
    fn a_second_drop_waits_out_the_holdoff() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        assert_eq!(window(&mut governor, 200), Some(7_000_000));

        // O encoder ainda não chegou à taxa nova: a perda é a mesma, e não é culpa nova.
        for _ in 0..HOLDOFF_WINDOWS {
            assert_eq!(window(&mut governor, 200), None, "queda dupla dentro da carência");
            assert_eq!(governor.loss_permille(), 200, "a perda segue medida e publicada");
        }

        assert_eq!(governor.target(), 7_000_000);
        assert_eq!(window(&mut governor, 200), Some(4_900_000), "passada a carência, a queda vale");
    }

    #[test]
    fn holdoff_windows_do_not_count_as_clean() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        window(&mut governor, 200);

        // Oito da carência e quatro limpas de verdade: ainda falta uma.
        for _ in 0..HOLDOFF_WINDOWS + CLEAN_WINDOWS - 1 {
            assert_eq!(window(&mut governor, 0), None);
        }

        assert_eq!(window(&mut governor, 0), Some(7_350_000));
    }

    #[test]
    fn light_random_loss_is_not_congestion() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        for _ in 0..30 {
            assert_eq!(window(&mut governor, 49), None, "4,9% é Wi-Fi ruim, não uplink cheio");
        }

        assert_eq!(governor.target(), CEILING);
    }

    #[test]
    fn the_target_never_goes_below_the_floor() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        for _ in 0..50 {
            window(&mut governor, 1000);
        }

        assert_eq!(governor.target(), 3_500_000);
        assert_eq!(window(&mut governor, 1000), None, "no piso não há mudança a anunciar");
    }

    #[test]
    fn a_still_screen_decides_nothing() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        // Vinte pacotes e todos perdidos: numa tela parada isso é um soluço, não uma medida.
        assert_eq!(governor.observe(20, 20, 0), None);
        assert_eq!(governor.target(), CEILING);
        assert_eq!(governor.loss_permille(), 0);

        // O que a janela trouxe não se perde: soma com a próxima que tiver tráfego.
        assert_eq!(governor.observe(180, 0, 0), Some(7_000_000));
        assert_eq!(governor.loss_permille(), 100);
    }

    #[test]
    fn the_target_climbs_slowly_after_five_clean_windows() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        window(&mut governor, 200);

        for _ in 0..HOLDOFF_WINDOWS + 4 {
            assert_eq!(window(&mut governor, 0), None);
        }

        assert_eq!(window(&mut governor, 9), Some(7_350_000));

        // Uma janela no meio do caminho (entre 1% e 5%) mantém, e zera a contagem.
        for _ in 0..4 {
            window(&mut governor, 0);
        }

        assert_eq!(window(&mut governor, 20), None);

        for _ in 0..4 {
            assert_eq!(window(&mut governor, 0), None);
        }

        assert_eq!(window(&mut governor, 0), Some(7_717_500));
    }

    #[test]
    fn the_target_stops_at_the_ceiling() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        window(&mut governor, 200);

        for _ in 0..100 {
            window(&mut governor, 0);
        }

        assert_eq!(governor.target(), CEILING);
    }

    #[test]
    fn dropped_packets_count_once_even_when_nacked_too() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        // 30 largados pelo buffer, e os mesmos 30 pedidos de volta: 3%, não 6%.
        assert_eq!(governor.observe(1000, 30, 30), None);
        assert_eq!(governor.loss_permille(), 30);

        // Sem caminho de volta (servidor sem chave), o buffer cheio ainda é sinal.
        assert_eq!(governor.observe(1000, 0, 60), Some(7_000_000));
    }

    #[test]
    fn off_measures_but_never_changes_the_rate() {
        let mut governor = BitrateGovernor::new(CEILING, false);

        assert_eq!(window(&mut governor, 300), None);
        assert_eq!(governor.target(), CEILING);
        assert_eq!(governor.loss_permille(), 300);
    }

    #[test]
    fn a_refused_encoder_goes_back_to_the_fixed_rate() {
        let mut governor = BitrateGovernor::new(CEILING, true);

        window(&mut governor, 300);
        governor.give_up();

        assert_eq!(governor.target(), CEILING);
        assert_eq!(window(&mut governor, 300), None);
    }
}
