//! Monta o MP4 de um pedaço do buffer do replay, sem recomprimir nada: o H.264 que o
//! encoder da placa já produziu só muda de embalagem. Salvar cinco minutos custa ler e
//! escrever uns 1,3 GB, e não uma nova codificação.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, bail};
use mp4::{
    AacConfig, AudioObjectType, AvcConfig, ChannelConfig, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer,
    SampleFreqIndex, TrackConfig, TrackType,
};

use crate::aac::{BITRATE, FRAME_SAMPLES, SAMPLE_RATE};
use crate::replay::{Record, Segment, Track, read_records};

/// A escala de tempo de vídeo que todo player conhece: 90 kHz.
const VIDEO_TIMESCALE: u32 = 90_000;

const VIDEO_TRACK: u32 = 1;
const AUDIO_TRACK: u32 = 2;

/// Meio quadro de AAC. O som entra a partir do quadro mais perto do começo do vídeo: o erro
/// de sincronia fica abaixo de 11 ms, longe dos ~45 ms em que alguém percebe.
const HALF_AUDIO_FRAME_NS: u64 = FRAME_SAMPLES as u64 * 500_000_000 / SAMPLE_RATE as u64;

pub struct ClipSummary {
    pub duration: Duration,
    pub video_frames: u64,
    pub audio_frames: u64,
    pub bytes: u64,
}

/// Escreve em `output` tudo do buffer a partir do último quadro-chave em ou antes de
/// `from_ns` — o clipe começa até 2 s antes do pedido, nunca depois.
///
/// ponytail: o tamanho do vídeo é o da captura atual; se a resolução do monitor mudou dentro
/// da janela do replay, o começo do clipe sai com o tamanho novo no cabeçalho (os players
/// seguem o SPS e decodificam certo, só a proporção declarada fica errada). Ler largura e
/// altura do próprio SPS resolve, se isso um dia aparecer.
pub fn write_clip(
    segments: &[Segment],
    from_ns: u64,
    size: (u32, u32),
    frame_rate: u32,
    output: &Path,
) -> anyhow::Result<ClipSummary> {
    let first = segments.iter().rposition(|segment| segment.start_ns <= from_ns).unwrap_or(0);
    let Some(start) = find_start(&segments[first..], from_ns)? else {
        bail!("o buffer ainda não tem nenhum quadro-chave");
    };

    let file = File::create(output).with_context(|| format!("criando {}", output.display()))?;
    let mut writer = Mp4Writer::write_start(
        BufWriter::with_capacity(1 << 20, file),
        &Mp4Config {
            major_brand: "isom".parse()?,
            minor_version: 512,
            compatible_brands: vec!["isom".parse()?, "iso2".parse()?, "avc1".parse()?, "mp41".parse()?],
            timescale: 1_000,
        },
    )?;

    writer.add_track(&TrackConfig {
        track_type: TrackType::Video,
        timescale: VIDEO_TIMESCALE,
        language: "und".into(),
        media_conf: MediaConfig::AvcConfig(AvcConfig {
            width: size.0 as u16,
            height: size.1 as u16,
            seq_param_set: start.sequence_parameters,
            pic_param_set: start.picture_parameters,
        }),
    })?;

    if start.has_audio {
        writer.add_track(&TrackConfig {
            track_type: TrackType::Audio,
            timescale: SAMPLE_RATE,
            language: "und".into(),
            media_conf: MediaConfig::AacConfig(AacConfig {
                bitrate: BITRATE,
                profile: AudioObjectType::AacLowComplexity,
                freq_index: SampleFreqIndex::Freq48000,
                chan_conf: ChannelConfig::Stereo,
            }),
        })?;
    }

    let mut video = TrackWriter::new(VIDEO_TRACK, VIDEO_TIMESCALE, start.timestamp_ns);
    let mut audio = TrackWriter::new(AUDIO_TRACK, SAMPLE_RATE, start.timestamp_ns);
    let mut last_ns = start.timestamp_ns;

    for segment in &segments[first..] {
        for record in read_records(segment)? {
            let record = record?;

            match record.track {
                Track::Video if record.timestamp_ns >= start.timestamp_ns => {
                    last_ns = last_ns.max(record.timestamp_ns);
                    video.push(&mut writer, record.timestamp_ns, record.keyframe, length_prefixed(&record.data))?;
                }
                Track::Audio if start.has_audio && record.timestamp_ns + HALF_AUDIO_FRAME_NS >= start.timestamp_ns => {
                    audio.push(&mut writer, record.timestamp_ns, true, record.data)?;
                }
                _ => {}
            }
        }
    }

    video.finish(&mut writer, VIDEO_TIMESCALE / frame_rate.max(1))?;
    audio.finish(&mut writer, FRAME_SAMPLES)?;
    writer.write_end()?;

    Ok(ClipSummary {
        duration: Duration::from_nanos(last_ns - start.timestamp_ns),
        video_frames: video.samples,
        audio_frames: audio.samples,
        bytes: std::fs::metadata(output)?.len(),
    })
}

struct Start {
    timestamp_ns: u64,

    /// Se o buffer tem som. Decide se o MP4 ganha trilha de áudio, e a trilha tem de existir
    /// antes da primeira amostra.
    has_audio: bool,
    sequence_parameters: Vec<u8>,
    picture_parameters: Vec<u8>,
}

/// O último quadro-chave em ou antes de `from_ns` no primeiro arquivo, ou o primeiro de
/// todos quando o pedido é mais velho que o buffer inteiro.
fn find_start(segments: &[Segment], from_ns: u64) -> anyhow::Result<Option<Start>> {
    let mut found: Option<Start> = None;
    let mut has_audio = false;

    for segment in segments {
        for record in read_records(segment)? {
            let record: Record = record?;

            has_audio |= record.track == Track::Audio;

            if let Some(start) = found.as_mut() {
                start.has_audio |= has_audio;
            }

            if record.track != Track::Video || !record.keyframe {
                continue;
            }

            if found.is_some() && record.timestamp_ns > from_ns {
                return Ok(found);
            }

            let mut sequence_parameters = None;
            let mut picture_parameters = None;

            for unit in nal_units(&record.data) {
                match unit.first().map(|byte| byte & 0x1F) {
                    Some(7) => sequence_parameters = Some(unit.to_vec()),
                    Some(8) => picture_parameters = Some(unit.to_vec()),
                    _ => {}
                }
            }

            if let (Some(sequence_parameters), Some(picture_parameters)) = (sequence_parameters, picture_parameters) {
                found = Some(Start { timestamp_ns: record.timestamp_ns, has_audio, sequence_parameters, picture_parameters });
            }
        }

        if found.is_some() {
            return Ok(found);
        }
    }

    Ok(found)
}

/// Uma trilha do MP4 com um quadro de atraso: a duração de cada amostra é a distância até a
/// próxima, e só se sabe quando a próxima chega. A captura só entrega quadro quando a tela
/// muda, então o intervalo varia — com duração fixa, uma tela parada encurtaria o vídeo.
struct TrackWriter {
    track: u32,
    timescale: u32,
    origin_ns: u64,
    pending: Option<(u64, bool, Vec<u8>)>,
    samples: u64,
}

impl TrackWriter {
    fn new(track: u32, timescale: u32, origin_ns: u64) -> Self {
        Self { track, timescale, origin_ns, pending: None, samples: 0 }
    }

    fn ticks(&self, timestamp_ns: u64) -> u64 {
        (timestamp_ns.saturating_sub(self.origin_ns) as u128 * u128::from(self.timescale) / 1_000_000_000) as u64
    }

    fn push(
        &mut self,
        writer: &mut Mp4Writer<BufWriter<File>>,
        timestamp_ns: u64,
        sync: bool,
        bytes: Vec<u8>,
    ) -> anyhow::Result<()> {
        let ticks = self.ticks(timestamp_ns);

        if let Some((start, pending_sync, pending_bytes)) = self.pending.take() {
            // Dois quadros no mesmo tick (relógio da captura com resolução maior que a do
            // MP4) viram duração 1, não zero: amostra de duração zero confunde os players.
            let duration = ticks.saturating_sub(start).max(1) as u32;

            self.write(writer, start, duration, pending_sync, pending_bytes)?;
        }

        self.pending = Some((ticks, sync, bytes));

        Ok(())
    }

    fn finish(&mut self, writer: &mut Mp4Writer<BufWriter<File>>, last_duration: u32) -> anyhow::Result<()> {
        if let Some((start, sync, bytes)) = self.pending.take() {
            self.write(writer, start, last_duration, sync, bytes)?;
        }

        Ok(())
    }

    fn write(
        &mut self,
        writer: &mut Mp4Writer<BufWriter<File>>,
        start: u64,
        duration: u32,
        sync: bool,
        bytes: Vec<u8>,
    ) -> anyhow::Result<()> {
        writer.write_sample(
            self.track,
            &Mp4Sample { start_time: start, duration, rendering_offset: 0, is_sync: sync, bytes: bytes.into() },
        )?;
        self.samples += 1;

        Ok(())
    }
}

/// Annex-B (`00 00 01` entre as unidades) para o formato do MP4 (tamanho de 4 bytes na frente
/// de cada uma). SPS e PPS saem: moram no cabeçalho da trilha. O delimitador de acesso (9)
/// também, porque no MP4 a amostra já é o quadro.
fn length_prefixed(annex_b: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(annex_b.len());

    for unit in nal_units(annex_b) {
        if matches!(unit.first().map(|byte| byte & 0x1F), Some(7..=9)) {
            continue;
        }

        output.extend_from_slice(&(unit.len() as u32).to_be_bytes());
        output.extend_from_slice(unit);
    }

    output
}

/// As unidades NAL de um trecho Annex-B, sem os prefixos. O prefixo de 4 bytes é o de 3 com
/// um zero na frente; esse zero ficaria no fim da unidade anterior, então sai junto com os
/// zeros de enchimento.
pub fn nal_units(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut starts = Vec::new();
    let mut position = 0;

    while position + 3 <= data.len() {
        if data[position..position + 3] == [0, 0, 1] {
            starts.push(position);
            position += 3;
        } else {
            position += 1;
        }
    }

    let ends: Vec<usize> = starts.iter().skip(1).copied().chain(std::iter::once(data.len())).collect();

    starts.into_iter().zip(ends).filter_map(move |(start, end)| {
        let mut unit = &data[start + 3..end];

        while let [rest @ .., 0] = unit {
            unit = rest;
        }

        (!unit.is_empty()).then_some(unit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_annex_b_with_both_prefix_sizes() {
        let data = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4, 5, 0];
        let units: Vec<&[u8]> = nal_units(&data).collect();

        assert_eq!(units, vec![&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 4, 5][..]]);
    }

    #[test]
    fn mp4_samples_keep_only_the_picture() {
        let data = [0, 0, 0, 1, 0x09, 0xF0, 0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2, 0, 0, 0, 1, 0x65, 9, 9];

        assert_eq!(length_prefixed(&data), vec![0, 0, 0, 3, 0x65, 9, 9]);
    }
}
