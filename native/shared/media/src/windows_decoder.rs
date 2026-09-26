//! Decodificador de H.264 no Windows: o Media Foundation do próprio sistema, para o app
//! assistir sem GStreamer.
//!
//! É o par do `unpack.rs`: ele remonta o quadro Annex-B que chegou pela rede, e isto o
//! transforma em pixels que a interface desenha. O MFT é o da Microsoft, que vem em todo
//! Windows; nenhum driver de placa precisa estar certo para alguém conseguir assistir.
//!
//! ponytail: decodifica na CPU, e a conversão de NV12 para RGB também é na CPU. Uma tela
//! 1080p60 custa uma fração de um núcleo — e quem assiste não está jogando. Teto: se pesar,
//! o gerente de device do Direct3D põe o MFT no DXVA e a conversão vira um VideoProcessor,
//! com uma cópia da textura para a interface no fim.

use std::mem::ManuallyDrop;

use ::windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFMediaType, IMFSample, IMFTransform, MF_E_NOTACCEPTING,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_MINIMUM_DISPLAY_APERTURE,
    MF_MT_SUBTYPE, MF_MT_YUV_MATRIX, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video,
    MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO,
    MFTEnumEx, MFVideoArea, MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoTransferMatrix_BT601,
};
use ::windows::Win32::System::Com::CoTaskMemFree;
use anyhow::{Context, Result, anyhow};

use crate::DecodedFrame;
use crate::windows::start_media_foundation;

/// O relógio do RTP para vídeo, e a unidade de tempo do Media Foundation (100 ns).
const RTP_CLOCK: i64 = 90_000;
const HNS_PER_SECOND: i64 = 10_000_000;

/// Quantas vezes seguidas o MFT pode mudar o formato de saída antes de desistir do quadro.
/// Ele muda uma vez, no primeiro quadro; em laço, é MFT quebrado, não vídeo novo.
const MOST_STREAM_CHANGES: u32 = 4;

/// Como o MFT entrega o NV12: o que se vê, e a geometria do buffer por trás.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Layout {
    width: u32,
    height: u32,
    /// Bytes por linha, com o alinhamento que o MFT escolheu.
    stride: u32,
    /// Linhas do plano Y no buffer: 1088 para um vídeo de 1080, por exemplo.
    rows: u32,
    matrix: Matrix,
}

/// A matriz de cor que o vídeo declara. HD sai em BT.709 dos nossos encoders; o BT.601 é
/// de quem manda vídeo SD, e lido com a matriz errada o amarelo puxa para o verde.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Matrix {
    #[default]
    Bt709,
    Bt601,
}

impl Matrix {
    /// Os coeficientes de faixa limitada multiplicados por 256: `(Y, R de V, G de U, G de V,
    /// B de U)`. Y é 255/219, e o resto sai da matriz de cada norma.
    const fn coefficients(self) -> (i32, i32, i32, i32, i32) {
        match self {
            Self::Bt709 => (298, 459, 55, 136, 541),
            Self::Bt601 => (298, 409, 100, 208, 516),
        }
    }
}

pub struct H264Decoder {
    transform: IMFTransform,
    layout: Layout,
}

// O MFT de software é free-threaded, e o decodificador só é usado por uma thread por vez:
// a da fila de mídia de quem assiste.
unsafe impl Send for H264Decoder {}

impl H264Decoder {
    pub fn new() -> Result<Self> {
        unsafe {
            start_media_foundation().map_err(|failure| anyhow!("{failure}"))?;

            let transform = open_decoder()?;

            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .context("o decodificador recusou o começo do streaming")?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .context("o decodificador recusou o começo do vídeo")?;

            let mut decoder = Self {
                transform,
                layout: Layout::default(),
            };

            decoder.choose_output()?;

            Ok(decoder)
        }
    }

    /// Entrega um quadro Annex-B e devolve o que ficou pronto. Com a baixa latência ligada o
    /// MFT devolve o próprio quadro na hora; a lista existe para o primeiro, que às vezes só
    /// sai junto com o segundo.
    pub fn decode(&mut self, annex_b: &[u8], timestamp: u32) -> Result<Vec<DecodedFrame>> {
        unsafe {
            let sample = input_sample(annex_b, timestamp)?;
            let mut ready = Vec::new();

            if let Err(failure) = self.transform.ProcessInput(0, &sample, 0) {
                if failure.code() != MF_E_NOTACCEPTING {
                    return Err(anyhow!(failure).context("o decodificador recusou o quadro"));
                }

                // Cheio: o que estava pronto sai primeiro, e aí o quadro entra.
                self.drain(&mut ready)?;
                self.transform
                    .ProcessInput(0, &sample, 0)
                    .context("o decodificador recusou o quadro depois de esvaziar")?;
            }

            self.drain(&mut ready)?;

            Ok(ready)
        }
    }

    unsafe fn drain(&mut self, ready: &mut Vec<DecodedFrame>) -> Result<()> {
        let mut changes = 0;

        loop {
            match unsafe { self.next_output()? } {
                Output::Frame(frame) => ready.push(frame),
                Output::Empty => return Ok(()),
                Output::StreamChanged => {
                    changes += 1;

                    if changes > MOST_STREAM_CHANGES {
                        return Err(anyhow!("o decodificador mudou de formato {changes} vezes seguidas"));
                    }

                    unsafe { self.choose_output()? };
                }
            }
        }
    }

    unsafe fn next_output(&mut self) -> Result<Output> {
        unsafe {
            let info = self
                .transform
                .GetOutputStreamInfo(0)
                .context("o decodificador não disse o tamanho da saída")?;
            let mut output = [MFT_OUTPUT_DATA_BUFFER::default()];
            let mut status = 0_u32;

            if info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 == 0 {
                let size = info.cbSize.max(self.layout.stride * self.layout.rows * 3 / 2).max(1);
                let buffer = MFCreateMemoryBuffer(size).context("sem memória para o quadro")?;
                let sample = MFCreateSample().context("sem amostra para o quadro")?;

                sample.AddBuffer(&buffer).context("a amostra recusou o buffer")?;
                output[0].pSample = ManuallyDrop::new(Some(sample));
            }

            let result = self.transform.ProcessOutput(0, &mut output, &mut status);
            let sample = ManuallyDrop::take(&mut output[0].pSample);

            drop(ManuallyDrop::take(&mut output[0].pEvents));

            match result {
                Ok(()) => {}
                Err(failure) if failure.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(Output::Empty),
                Err(failure) if failure.code() == MF_E_TRANSFORM_STREAM_CHANGE => return Ok(Output::StreamChanged),
                Err(failure) => return Err(anyhow!(failure).context("o decodificador falhou no quadro")),
            }

            let sample = sample.ok_or_else(|| anyhow!("o decodificador não devolveu amostra"))?;

            Ok(Output::Frame(self.read(&sample)?))
        }
    }

    unsafe fn read(&self, sample: &IMFSample) -> Result<DecodedFrame> {
        unsafe {
            let buffer = sample
                .ConvertToContiguousBuffer()
                .context("o quadro decodificado não virou um buffer só")?;
            let mut start = std::ptr::null_mut();
            let mut size = 0_u32;

            buffer.Lock(&mut start, None, Some(&mut size)).context("o quadro não abriu para leitura")?;

            let nv12 = std::slice::from_raw_parts(start, size as usize);
            let converted = nv12_to_rgb(nv12, self.layout);

            buffer.Unlock().context("o quadro não fechou depois da leitura")?;

            converted
        }
    }

    /// Escolhe o NV12 entre as saídas que o MFT oferece e lê a geometria dele. Roda na
    /// abertura e de novo a cada `STREAM_CHANGE`, que é quando o tamanho de verdade chega
    /// (o MFT só o conhece depois de ler o SPS do primeiro keyframe).
    unsafe fn choose_output(&mut self) -> Result<()> {
        unsafe {
            for index in 0.. {
                let Ok(offered) = self.transform.GetOutputAvailableType(0, index) else {
                    break;
                };

                if offered.GetGUID(&MF_MT_SUBTYPE).ok() != Some(MFVideoFormat_NV12) {
                    continue;
                }

                self.transform
                    .SetOutputType(0, &offered, 0)
                    .context("o decodificador recusou o NV12 que ele mesmo ofereceu")?;
                self.layout = layout_of(&offered);

                return Ok(());
            }

            Err(anyhow!("o decodificador não oferece NV12"))
        }
    }
}

enum Output {
    Frame(DecodedFrame),
    Empty,
    StreamChanged,
}

/// O primeiro decodificador de H.264 que aceitar entrar. Só os síncronos: o assíncrono
/// pede eventos, e o da Microsoft — que todo Windows tem — é síncrono.
unsafe fn open_decoder() -> Result<IMFTransform> {
    unsafe {
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let mut found: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut how_many = 0_u32;

        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut found,
            &mut how_many,
        )
        .context("o Windows não listou os decodificadores")?;

        let candidates: Vec<IMFActivate> = if found.is_null() {
            Vec::new()
        } else {
            let taken = std::slice::from_raw_parts_mut(found, how_many as usize)
                .iter_mut()
                .filter_map(Option::take)
                .collect();

            CoTaskMemFree(Some(found.cast::<core::ffi::c_void>().cast_const()));

            taken
        };

        for activate in candidates {
            match try_decoder(&activate) {
                Ok(transform) => return Ok(transform),
                Err(failure) => {
                    tracing::warn!(%failure, "decodificador: MFT recusou, tentando o próximo");

                    let _ = activate.ShutdownObject();
                }
            }
        }

        Err(anyhow!("nenhum decodificador de H.264 desta máquina aceitou"))
    }
}

unsafe fn try_decoder(activate: &IMFActivate) -> Result<IMFTransform> {
    unsafe {
        let transform: IMFTransform = activate.ActivateObject().context("o MFT não ativou")?;

        // Sem isto o MFT segura quadros para reordenar, e quem assiste fica um quarto de
        // segundo atrás de quem transmite. Nosso H.264 não tem quadro B: não há o que esperar.
        if let Ok(attributes) = transform.GetAttributes() {
            let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
        }

        let input: IMFMediaType = MFCreateMediaType().context("sem tipo de mídia")?;

        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).context("tipo de vídeo")?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).context("subtipo H.264")?;
        transform.SetInputType(0, &input, 0).context("o MFT recusou H.264 na entrada")?;

        Ok(transform)
    }
}

unsafe fn input_sample(annex_b: &[u8], timestamp: u32) -> Result<IMFSample> {
    unsafe {
        let length = u32::try_from(annex_b.len()).context("quadro grande demais")?;
        let buffer = MFCreateMemoryBuffer(length.max(1)).context("sem memória para o quadro")?;
        let mut start = std::ptr::null_mut();

        buffer.Lock(&mut start, None, None).context("o buffer de entrada não abriu")?;
        std::ptr::copy_nonoverlapping(annex_b.as_ptr(), start, annex_b.len());
        buffer.Unlock().context("o buffer de entrada não fechou")?;
        buffer.SetCurrentLength(length).context("o buffer recusou o tamanho")?;

        let sample = MFCreateSample().context("sem amostra")?;

        sample.AddBuffer(&buffer).context("a amostra recusou o buffer")?;
        sample
            .SetSampleTime(i64::from(timestamp) * HNS_PER_SECOND / RTP_CLOCK)
            .context("a amostra recusou o tempo")?;

        Ok(sample)
    }
}

/// A geometria do NV12 que o MFT vai entregar. O que se vê vem da abertura mínima, quando
/// existe: o buffer de um vídeo 1080p tem 1088 linhas, e as 8 de baixo são enchimento.
unsafe fn layout_of(media_type: &IMFMediaType) -> Layout {
    unsafe {
        let packed = media_type.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
        #[allow(clippy::cast_possible_truncation)]
        let (width, rows) = ((packed >> 32) as u32, packed as u32);
        #[allow(clippy::cast_sign_loss)]
        let stride = media_type
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map_or(width, |stride| (stride as i32).unsigned_abs());

        let mut aperture = MFVideoArea::default();
        let visible = media_type
            .GetBlob(
                &MF_MT_MINIMUM_DISPLAY_APERTURE,
                std::slice::from_raw_parts_mut(
                    std::ptr::from_mut(&mut aperture).cast::<u8>(),
                    std::mem::size_of::<MFVideoArea>(),
                ),
                None,
            )
            .ok()
            .and_then(|()| {
                let (seen_width, seen_height) = (aperture.Area.cx, aperture.Area.cy);

                (seen_width > 0 && seen_height > 0).then(|| (seen_width.unsigned_abs(), seen_height.unsigned_abs()))
            });

        let (width, height) = visible.map_or((width, rows), |(seen_width, seen_height)| {
            (seen_width.min(width), seen_height.min(rows))
        });
        #[allow(clippy::cast_possible_wrap)]
        let matrix = match media_type.GetUINT32(&MF_MT_YUV_MATRIX) {
            Ok(declared) if declared as i32 == MFVideoTransferMatrix_BT601.0 => Matrix::Bt601,
            _ => Matrix::Bt709,
        };

        Layout { width, height, stride: stride.max(width), rows, matrix }
    }
}

/// NV12 de faixa limitada para RGB, na matriz que o vídeo declarou. Conta inteira em ponto
/// fixo de 8 bits: cabe num `i32` e não tem divisão nem arredondamento por pixel.
fn nv12_to_rgb(nv12: &[u8], layout: Layout) -> Result<DecodedFrame> {
    let Layout { width, height, stride, rows, matrix } = layout;
    let (luma_gain, red_v, green_u, green_v, blue_u) = matrix.coefficients();
    let (width, height, stride, rows) = (width as usize, height as usize, stride as usize, rows as usize);
    let chroma_start = stride * rows;
    let needed = chroma_start + stride * height.div_ceil(2);

    if width == 0 || height == 0 || nv12.len() < needed {
        return Err(anyhow!(
            "quadro de {} bytes não cabe em {width}x{height} com linha de {stride}",
            nv12.len()
        ));
    }

    let mut rgb = vec![0_u8; width * height * 3];

    for row in 0..height {
        let luma = &nv12[row * stride..row * stride + width];
        let chroma = &nv12[chroma_start + (row / 2) * stride..];
        let line = &mut rgb[row * width * 3..(row + 1) * width * 3];

        for column in 0..width {
            let y = (i32::from(luma[column]) - 16) * luma_gain;
            let u = i32::from(chroma[column & !1]) - 128;
            let v = i32::from(chroma[(column & !1) + 1]) - 128;
            let pixel = &mut line[column * 3..column * 3 + 3];

            pixel[0] = clamp((y + red_v * v + 128) >> 8);
            pixel[1] = clamp((y - green_u * u - green_v * v + 128) >> 8);
            pixel[2] = clamp((y + blue_u * u + 128) >> 8);
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    Ok(DecodedFrame {
        width: width as u32,
        height: height as u32,
        rgb,
    })
}

fn clamp(value: i32) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let clamped = value.clamp(0, 255) as u8;

    clamped
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/testsrc-320x240.h264");

    /// O arquivo de teste tem um AUD na frente de cada quadro, como o que chega da rede.
    fn access_units(stream: &[u8]) -> Vec<&[u8]> {
        let mut starts: Vec<usize> = stream
            .windows(4)
            .enumerate()
            .filter(|(_, window)| window[..3] == [0, 0, 1] && window[3] & 0x1f == 9)
            .map(|(index, _)| if index > 0 && stream[index - 1] == 0 { index - 1 } else { index })
            .collect();

        starts.push(stream.len());
        starts.windows(2).map(|pair| &stream[pair[0]..pair[1]]).collect()
    }

    #[test]
    fn the_fixture_is_split_into_its_six_frames() {
        assert_eq!(access_units(FIXTURE).len(), 6);
    }

    #[test]
    fn every_frame_of_a_real_stream_comes_out_as_pixels() {
        let mut decoder = H264Decoder::new().expect("o decodificador abriu");
        let mut frames = Vec::new();

        for (index, unit) in access_units(FIXTURE).into_iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let timestamp = index as u32 * 3_000;

            frames.extend(decoder.decode(unit, timestamp).expect("o quadro decodificou"));
        }

        assert_eq!(frames.len(), 6, "o MFT segurou quadro: saíram {}", frames.len());

        for frame in &frames {
            assert_eq!((frame.width, frame.height), (320, 240));
            assert_eq!(frame.rgb.len(), 320 * 240 * 3);
        }

        // As sete barras do `smpte` do GStreamer, da esquerda para a direita. Cor certa em
        // cada uma prova a matriz, e cada uma no lugar certo prova a geometria do buffer.
        let bars = [
            [255, 255, 255], [255, 255, 0], [0, 255, 255], [0, 255, 0],
            [255, 0, 255], [255, 0, 0], [0, 0, 255],
        ];

        for (index, expected) in bars.iter().enumerate() {
            let x = 320 * (2 * index + 1) / 14;
            let pixel = &frames[0].rgb[(20 * 320 + x) * 3..(20 * 320 + x) * 3 + 3];
            let far = pixel.iter().zip(expected).any(|(&got, &want)| (i32::from(got) - want).abs() > 12);

            assert!(!far, "a barra {index} saiu {pixel:?}, esperava {expected:?}");
        }
    }

    #[test]
    fn nv12_white_black_and_red_become_the_right_rgb() {
        // Um bloco 2x2 tem um par de croma só, então cada caso pinta o bloco inteiro.
        let white = Layout { width: 2, height: 2, stride: 2, rows: 2, matrix: Matrix::Bt709 };
        let frame = nv12_to_rgb(&[235, 235, 235, 235, 128, 128], white).unwrap();

        assert!(frame.rgb.iter().all(|&channel| channel >= 254), "{:?}", frame.rgb);

        let frame = nv12_to_rgb(&[16, 16, 16, 16, 128, 128], white).unwrap();

        assert!(frame.rgb.iter().all(|&channel| channel <= 1), "{:?}", frame.rgb);

        // Vermelho puro em BT.709 limitado: Y 63, U 102, V 240.
        let frame = nv12_to_rgb(&[63, 63, 63, 63, 102, 240], white).unwrap();
        let pixel = &frame.rgb[..3];

        assert!(pixel[0] >= 250 && pixel[1] <= 5 && pixel[2] <= 5, "{pixel:?}");
    }

    #[test]
    fn the_padding_rows_and_columns_are_left_out() {
        // Um vídeo 2x2 dentro de um buffer de 4 colunas e 4 linhas, como o MFT alinha.
        let layout = Layout { width: 2, height: 2, stride: 4, rows: 4, matrix: Matrix::Bt709 };
        let mut nv12 = vec![0_u8; 4 * 4 + 4 * 2];

        nv12[..2].copy_from_slice(&[235, 235]);
        nv12[4..6].copy_from_slice(&[235, 235]);
        nv12[16..18].copy_from_slice(&[128, 128]);

        let frame = nv12_to_rgb(&nv12, layout).unwrap();

        assert_eq!((frame.width, frame.height, frame.rgb.len()), (2, 2, 12));
        assert!(frame.rgb.iter().all(|&channel| channel >= 254), "{:?}", frame.rgb);
    }

    #[test]
    fn a_short_buffer_is_refused_instead_of_read_past_the_end() {
        let layout = Layout { width: 4, height: 4, stride: 4, rows: 4, matrix: Matrix::Bt709 };

        assert!(nv12_to_rgb(&[0; 10], layout).is_err());
    }
}
