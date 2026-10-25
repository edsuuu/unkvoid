//! O processamento do microfone, no núcleo para que a captura só entregue PCM e nunca espere
//! por um filtro de áudio.

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};

pub(crate) const SAMPLE_RATE: u32 = 48_000;
pub(crate) const CHANNELS: usize = 2;
/// Sonora e RNNoise consomem janelas de 10 ms nesta taxa, isto é, 480 amostras por canal.
pub(crate) const FRAME_SAMPLES: usize = 480;
const FRAME_LEN: usize = FRAME_SAMPLES * CHANNELS;
/// Limita o atraso a poucos callbacks; em vez de acumular latência, a fila descarta o mais antigo.
const AUDIO_QUEUE_BLOCKS: usize = 16;

/// Fila limitada por blocos para que a captura não espere pelo worker de áudio.
pub(crate) struct AudioQueue {
    state: Mutex<AudioQueueState>,
    ready: Condvar,
    dropped: AtomicU64,
    capacity: usize,
}

struct AudioQueueState {
    blocks: VecDeque<Vec<f32>>,
    closed: bool,
}

impl AudioQueue {
    pub(crate) fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            state: Mutex::new(AudioQueueState {
                blocks: VecDeque::with_capacity(capacity),
                closed: false,
            }),
            ready: Condvar::new(),
            dropped: AtomicU64::new(0),
            capacity,
        }
    }

    /// Copia o bloco e descarta o mais antigo quando o worker atrasar.
    pub(crate) fn push(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }

        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return;
        }

        if state.blocks.len() == self.capacity {
            state.blocks.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        state.blocks.push_back(samples.to_vec());
        self.ready.notify_one();
    }

    pub(crate) fn pop(&self) -> Option<Vec<f32>> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(block) = state.blocks.pop_front() {
                return Some(block);
            }
            if state.closed {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    pub(crate) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        state.blocks.clear();
        drop(state);
        self.ready.notify_one();
    }

    pub(crate) fn dropped_blocks(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

pub(crate) fn microphone_queue() -> AudioQueue {
    AudioQueue::new(AUDIO_QUEUE_BLOCKS)
}

/// A configuração mínima do pipeline enquanto só o perfil leve está implementado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceFilterConfig {
    rnnoise: bool,
    input_volume: u16,
}

impl Default for VoiceFilterConfig {
    fn default() -> Self {
        Self { rnnoise: false, input_volume: 100 }
    }
}

impl VoiceFilterConfig {
    /// Lê o objeto atual sem transformar opções que o núcleo ainda não implementa em filtros.
    pub fn from_preference(preference: Option<&Value>) -> Self {
        let Some(preference) = preference.and_then(Value::as_object) else {
            return Self::default();
        };

        if let Some(noise) = preference.get("noise") {
            return Self { rnnoise: noise.as_str() == Some("light"), input_volume: read_input_volume(preference) };
        }

        // A preferência antiga só significa RNNoise enquanto a opção DeepFilterNet não existe.
        Self { rnnoise: preference.get("noiseSuppress").and_then(Value::as_bool) == Some(true), input_volume: read_input_volume(preference) }
    }

    pub(crate) fn rnnoise(self) -> bool {
        self.rnnoise
    }

    pub(crate) fn input_volume(self) -> u16 {
        self.input_volume
    }
}

fn read_input_volume(preference: &serde_json::Map<String, Value>) -> u16 {
    u16::try_from(preference.get("inputVolume").and_then(Value::as_u64).unwrap_or(100).clamp(0, 200)).unwrap_or(200)
}

pub(crate) fn apply_input_gain(samples: &mut [f32], volume: u16) {
    let gain = f32::from(volume) / 100.0;
    for sample in samples {
        *sample = (*sample * gain).clamp(-1.0, 1.0);
    }
}

/// Junta pedaços variáveis em quadros estéreo de 10 ms, conservando a sobra entre chamadas.
#[derive(Default)]
pub(crate) struct FrameAssembler {
    pending: Vec<f32>,
}

impl FrameAssembler {
    pub(crate) fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.pending.extend_from_slice(samples);
        let complete = self.pending.len() / FRAME_LEN;

        if complete == 0 {
            return Vec::new();
        }

        let consumed = complete * FRAME_LEN;
        let remainder = self.pending.split_off(consumed);
        let frames = self.pending.as_chunks::<FRAME_LEN>().0.iter().map(|frame| frame.to_vec()).collect();
        self.pending = remainder;

        frames
    }
}

/// Estado contínuo do RNNoise: o modelo recebe mono escalado como PCM de 16 bits.
pub(crate) struct NoiseFilter {
    denoiser: Option<Box<nnnoiseless::DenoiseState<'static>>>,
}

impl NoiseFilter {
    pub(crate) fn new(config: VoiceFilterConfig) -> Self {
        Self {
            denoiser: config.rnnoise().then(nnnoiseless::DenoiseState::new),
        }
    }

    pub(crate) fn process(&mut self, stereo: &mut [f32]) {
        let Some(denoiser) = self.denoiser.as_mut() else {
            return;
        };

        debug_assert_eq!(stereo.len(), FRAME_LEN);
        let mut mono = [0.0_f32; FRAME_SAMPLES];
        let mut cleaned = [0.0_f32; FRAME_SAMPLES];

        for (sample, [left, right]) in mono.iter_mut().zip(stereo.as_chunks::<2>().0) {
            *sample = (left + right) * 0.5 * 32_767.0;
        }

        denoiser.process_frame(&mut cleaned, &mono);

        for (pair, sample) in stereo.as_chunks_mut::<2>().0.iter_mut().zip(cleaned) {
            pair[0] = sample / 32_767.0;
            pair[1] = sample / 32_767.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variable_blocks_make_complete_frames_without_losing_or_repeating_samples() {
        let samples: Vec<_> = (0..FRAME_LEN * 3 + 318)
            .map(|sample| sample as f32)
            .collect();
        let mut assembler = FrameAssembler::default();
        let mut frames = Vec::new();

        for block in [&samples[..316], &samples[316..2_000], &samples[2_000..]] {
            frames.extend(assembler.push(block));
        }

        let joined: Vec<_> = frames.iter().flatten().copied().collect();
        assert_eq!(joined.as_slice(), &samples[..FRAME_LEN * 3]);

        assert_eq!(assembler.pending, samples[FRAME_LEN * 3..]);

        let final_frame = assembler.push(&vec![-1.0; FRAME_LEN - 318]);
        assert_eq!(final_frame.len(), 1);
        assert_eq!(&final_frame[0][..318], &samples[FRAME_LEN * 3..]);
        assert!(final_frame[0][318..].iter().all(|sample| *sample == -1.0));
    }

    #[test]
    fn a_full_audio_queue_discards_the_oldest_block() {
        let queue = AudioQueue::new(2);

        queue.push(&[1.0]);
        queue.push(&[2.0]);
        queue.push(&[3.0]);
        assert_eq!(queue.dropped_blocks(), 1);
        assert_eq!(queue.pop(), Some(vec![2.0]));
        assert_eq!(queue.pop(), Some(vec![3.0]));
        queue.close();
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn rnnoise_reduces_reproducible_white_noise_and_preserves_silence() {
        // Meio segundo de ruído: o primeiro quadro do RNNoise sai quase vazio enquanto o modelo
        // aquece, e medir só ele aprovaria um filtro que não filtra nada.
        let mut seed = 0x4d59_5df4_u32;
        let mut noise = vec![0.0_f32; FRAME_LEN * 50];
        for sample in &mut noise {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *sample = ((seed >> 8) as f32 / 16_777_215.0 - 0.5) * 0.02;
        }

        let mut filter = NoiseFilter::new(VoiceFilterConfig { rnnoise: true, input_volume: 100 });
        let mut filtered = noise.clone();
        for frame in filtered.as_chunks_mut::<FRAME_LEN>().0 {
            filter.process(frame);
        }

        let (filtered, input_energy) = (&filtered[FRAME_LEN * 10..], energy(&noise[FRAME_LEN * 10..]));
        assert!(energy(filtered) < input_energy * 0.8, "ruído: {} → {}", input_energy, energy(filtered));

        let mut silence_filter = NoiseFilter::new(VoiceFilterConfig { rnnoise: true, input_volume: 100 });
        let mut silence = vec![0.0_f32; FRAME_LEN];
        silence_filter.process(&mut silence);
        assert!(silence.iter().all(|sample| sample.abs() < 1e-7));
    }

    #[test]
    fn noise_preference_supports_light_off_unknown_and_the_legacy_flag() {
        assert!(!VoiceFilterConfig::from_preference(None).rnnoise());
        assert!(
            !VoiceFilterConfig::from_preference(Some(&serde_json::json!({ "noise": "off" })))
                .rnnoise()
        );
        assert!(
            VoiceFilterConfig::from_preference(Some(&serde_json::json!({ "noise": "light" })))
                .rnnoise()
        );
        assert!(
            !VoiceFilterConfig::from_preference(Some(&serde_json::json!({ "noise": "deep" })))
                .rnnoise()
        );
        assert!(
            !VoiceFilterConfig::from_preference(Some(&serde_json::json!({ "noise": "unknown" })))
                .rnnoise()
        );
        assert!(
            VoiceFilterConfig::from_preference(Some(&serde_json::json!({ "noiseSuppress": true })))
                .rnnoise()
        );
        assert!(
            !VoiceFilterConfig::from_preference(Some(
                &serde_json::json!({ "noiseSuppress": false })
            ))
            .rnnoise()
        );
        assert!(
            !VoiceFilterConfig::from_preference(Some(
                &serde_json::json!({ "noise": "off", "noiseSuppress": true })
            ))
            .rnnoise()
        );
    }

    #[test]
    fn input_gain_clamps_and_one_hundred_percent_preserves_samples() {
        let mut unchanged = [-0.75, -0.1, 0.0, 0.4, 1.0];
        let original = unchanged;
        apply_input_gain(&mut unchanged, 100);
        assert_eq!(unchanged, original);

        let mut amplified = [-0.75, 0.4, 0.75];
        apply_input_gain(&mut amplified, 200);
        assert_eq!(amplified, [-1.0, 0.8, 1.0]);
    }

    #[test]
    fn input_volume_preference_defaults_and_clamps_to_supported_range() {
        assert_eq!(VoiceFilterConfig::from_preference(None).input_volume(), 100);
        assert_eq!(VoiceFilterConfig::from_preference(Some(&serde_json::json!({ "inputVolume": 0 }))).input_volume(), 0);
        assert_eq!(VoiceFilterConfig::from_preference(Some(&serde_json::json!({ "inputVolume": 250 }))).input_volume(), 200);
    }

    fn energy(samples: &[f32]) -> f64 {
        samples
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum::<f64>()
            / samples.len() as f64
    }
}
