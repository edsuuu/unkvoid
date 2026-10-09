//! AAC pelo encoder que já vem no Windows (o MFT de AAC do Media Foundation).
//!
//! AAC e não Opus: é o que o Discord, o WhatsApp e qualquer player tocam dentro de um MP4
//! sem pedir nada. Um quadro de AAC são 1024 amostras por canal — 21,3 ms a 48 kHz.

use anyhow::Context;
use windows::Win32::Media::MediaFoundation::{
    AACMFTEncoder, IMFMediaType, IMFTransform, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT,
    MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MFAudioFormat_AAC, MFAudioFormat_PCM, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Audio, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_OUTPUT_DATA_BUFFER,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;
pub const FRAME_SAMPLES: u32 = 1_024;

/// 192 kb/s. O encoder do Windows só aceita 96, 128, 160 e 192; o mais alto é o que deixa
/// música e explosão sem aquele chiado de AAC apertado, e custa 1,4 MB por minuto.
pub const BITRATE: u32 = 192_000;

pub struct AacEncoder {
    transform: IMFTransform,
    output_size: u32,
}

/// Vive e morre na thread da mistura; o `Send` é só para entrar nela.
unsafe impl Send for AacEncoder {}

pub struct AacFrame {
    pub timestamp_ns: u64,
    pub data: Vec<u8>,
}

impl AacEncoder {
    /// Precisa do COM iniciado na thread que chama.
    pub fn new() -> anyhow::Result<Self> {
        unsafe {
            crate::encoder::start_media_foundation()?;

            let transform: IMFTransform = CoCreateInstance(&AACMFTEncoder, None, CLSCTX_INPROC_SERVER)
                .context("o encoder de AAC do Windows não abriu")?;
            let output: IMFMediaType = MFCreateMediaType()?;

            output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            output.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            output.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            output.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, SAMPLE_RATE)?;
            output.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS)?;
            output.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, BITRATE / 8)?;
            transform.SetOutputType(0, Some(&output), 0)?;

            let input: IMFMediaType = MFCreateMediaType()?;

            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            input.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, SAMPLE_RATE)?;
            input.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS)?;
            input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, CHANNELS * 2)?;
            input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, SAMPLE_RATE * CHANNELS * 2)?;
            transform.SetInputType(0, Some(&input), 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            let output_size = transform.GetOutputStreamInfo(0)?.cbSize.max(8_192);

            Ok(Self { transform, output_size })
        }
    }

    /// Entrega PCM de 16 bits intercalado (esquerdo, direito) que começa em `timestamp_ns` e
    /// devolve os quadros de AAC que ficaram prontos.
    pub fn encode(&mut self, pcm: &[i16], timestamp_ns: u64) -> anyhow::Result<Vec<AacFrame>> {
        unsafe {
            let bytes = std::mem::size_of_val(pcm) as u32;
            let buffer = MFCreateMemoryBuffer(bytes)?;
            let mut start = std::ptr::null_mut();

            buffer.Lock(&mut start, None, None)?;
            std::ptr::copy_nonoverlapping(pcm.as_ptr().cast::<u8>(), start, bytes as usize);
            buffer.Unlock()?;
            buffer.SetCurrentLength(bytes)?;

            let sample = MFCreateSample()?;
            let frames = pcm.len() as u64 / u64::from(CHANNELS);

            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime((timestamp_ns / 100) as i64)?;
            sample.SetSampleDuration((frames * 10_000_000 / u64::from(SAMPLE_RATE)) as i64)?;
            self.transform.ProcessInput(0, &sample, 0)?;

            let mut frames_out = Vec::new();

            loop {
                let sample = MFCreateSample()?;

                sample.AddBuffer(&MFCreateMemoryBuffer(self.output_size)?)?;

                let mut output = [MFT_OUTPUT_DATA_BUFFER {
                    pSample: std::mem::ManuallyDrop::new(Some(sample)),
                    ..Default::default()
                }];
                let mut status = 0_u32;
                let result = self.transform.ProcessOutput(0, &mut output, &mut status);
                let sample = output[0].pSample.take();

                match result {
                    Ok(()) => {}
                    Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                    Err(error) => return Err(error.into()),
                }

                let sample = sample.context("o encoder de AAC não devolveu amostra")?;
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut start = std::ptr::null_mut();
                let mut size = 0_u32;

                buffer.Lock(&mut start, None, Some(&mut size))?;

                let data = std::slice::from_raw_parts(start, size as usize).to_vec();

                buffer.Unlock()?;
                frames_out.push(AacFrame {
                    timestamp_ns: crate::clock::from_hundred_nanoseconds(sample.GetSampleTime()?),
                    data,
                });
            }

            Ok(frames_out)
        }
    }
}
