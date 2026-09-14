use core::ffi::c_void;
use core::{ptr, slice};

use apple_cf::cm::CMFormatDescription;
use apple_cf::iosurface::IOSurface;
use apple_cf::raw;
use videotoolbox::ffi;
use videotoolbox::prelude::*;

use crate::{EncodedFrame, EncoderConfig, EncoderError, FramePacer, cpu_forced};

/// O separador de NAL do Annex-B.
const START_CODE: [u8; 4] = [0, 0, 0, 1];

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
        video_desc: *const c_void,
        index: usize,
        pointer_out: *mut *const u8,
        size_out: *mut usize,
        count_out: *mut usize,
        nal_header_length_out: *mut i32,
    ) -> i32;
}

/// Encoder H.264 do VideoToolbox. No Apple Silicon ele roda no chip de mídia — o
/// processador só entrega o buffer e recebe os bytes de volta.
pub struct VideoToolboxEncoder {
    session: CompressionSession,

    /// Só existe quando o VideoToolbox caiu para software: é quem segura os 30 fps.
    pacer: Option<FramePacer>,
}

impl VideoToolboxEncoder {
    /// Sem especificação de encoder o VideoToolbox escolhe o chip de mídia quando há —
    /// inclusive o QuickSync de um Mac Intel — e cai sozinho para software quando não há.
    /// Exigir hardware recusava transmitir numa máquina que transmitiria pior, e o dono
    /// decidiu que pior é melhor do que nada. Saber qual veio decide só o teto.
    ///
    /// ponytail: nunca rodou num Mac. Em software a captura continua no tamanho pedido e o
    /// VideoToolbox escala na entrada; reabrir a captura em 720p pouparia essa escala.
    pub fn new(config: &EncoderConfig) -> Result<Self, EncoderError> {
        if cpu_forced() {
            return Self::software(config);
        }

        let session = open_session(config)?;

        // SEGURANÇA: a chave é o CFString estático do SDK; a sessão acabou de nascer.
        let hardware = match unsafe {
            session.copy_property(ffi::kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder)
        } {
            // O `kCFBooleanTrue` é um objeto único do processo: comparar o ponteiro basta.
            Ok(Some(value)) => value.as_ptr() as *const c_void == unsafe { raw::kCFBooleanTrue } as *const c_void,
            // Sistema que não responde fica com a placa: rebaixar um Mac bom por uma
            // pergunta sem resposta custaria mais do que o risco.
            Ok(None) | Err(_) => {
                tracing::warn!("encoder: o VideoToolbox não disse se usa hardware; seguindo como se usasse");

                true
            }
        };

        if !hardware {
            drop(session);

            return Self::software(config);
        }

        tracing::info!("encoder: VideoToolbox por hardware");

        Ok(Self { session, pacer: None })
    }

    fn software(config: &EncoderConfig) -> Result<Self, EncoderError> {
        let config = config.for_cpu();

        tracing::warn!(
            width = config.width,
            height = config.height,
            frame_rate = config.frame_rate,
            "encoder: VideoToolbox por software, com o teto do processador"
        );

        Ok(Self {
            session: open_session(&config)?,
            pacer: Some(FramePacer::new(config.frame_rate)),
        })
    }

    /// Se o H.264 sai do chip de mídia.
    pub fn hardware(&self) -> bool {
        self.pacer.is_none()
    }
}

fn open_session(config: &EncoderConfig) -> Result<CompressionSession, EncoderError> {
    let session = CompressionSession::builder(config.width as i32, config.height as i32, Codec::H264)
        // Real time: prioritize low latency over compression ratio.
        .with_real_time(true)
        // Sem quadros B. Eles comprimem melhor, mas exigem reordenar quadros, o que
        // acrescenta latência — inaceitável numa chamada.
        .with_allow_frame_reordering(false)
        .with_average_bit_rate(config.bitrate as i32)
        .with_expected_frame_rate(config.frame_rate)
        // Um keyframe por segundo: um fragmento RTP perdido se recupera rápido, em
        // vez de congelar quem assiste até um GOP de dois segundos fechar.
        .with_max_keyframe_interval(config.frame_rate as i32)
        .build()
        .map_err(|error| EncoderError::Start(error.to_string()))?;

    // BT.709 declarado no SPS. Sem isto o VideoToolbox não escreve VUI, e o
    // decodificador de quem assiste chuta a matriz — e a captura BGRA do
    // ScreenCaptureKit é sRGB, que em vídeo é 709. Não é fatal: encoder que recusa a
    // propriedade continua codificando, só sem avisar.
    for (name, key, value) in [
        ("primaries", unsafe { ffi::kVTCompressionPropertyKey_ColorPrimaries }, unsafe { raw::kCMFormatDescriptionColorPrimaries_ITU_R_709_2 }),
        ("transfer", unsafe { ffi::kVTCompressionPropertyKey_TransferFunction }, unsafe { raw::kCMFormatDescriptionTransferFunction_ITU_R_709_2 }),
        ("matrix", unsafe { ffi::kVTCompressionPropertyKey_YCbCrMatrix }, unsafe { raw::kCMFormatDescriptionYCbCrMatrix_ITU_R_709_2 }),
    ] {
        // SEGURANÇA: chave e valor são os CFStrings estáticos do SDK, válidos pelo
        // processo inteiro; a sessão é a que acabou de nascer.
        if let Err(error) = unsafe { session.set_property(key, value.cast()) } {
            tracing::warn!(error = %error, ajuste = name, "encoder: cor recusada");
        }
    }

    Ok(session)
}

impl VideoToolboxEncoder {
    /// O servidor pediu um quadro-chave.
    ///
    /// ponytail: sem efeito. `kVTEncodeFrameOptionKey_ForceKeyFrame` é opção por quadro
    /// (o dicionário de `VTCompressionSessionEncodeFrame`), e o `encode` síncrono do
    /// `videotoolbox` 0.19 passa `NULL` ali sem alternativa: o `VTCompressionSessionRef`
    /// é privado e a variante que aceita o dicionário é só a `async`. O intervalo de um
    /// segundo é o teto da travada até a crate abrir isso.
    pub fn request_keyframe(&mut self) {}

    /// Codifica um quadro. A `surface` vem da captura sem passar pelo processador.
    ///
    /// O VideoToolbox devolve AVCC: cada NAL vem com um prefixo de tamanho, e SPS/PPS
    /// ficam guardados na descrição de formato, nunca no meio dos bytes. O empacotador
    /// RTP lá na frente só entende Annex-B e só descobre os parameter sets se eles
    /// passarem por ele. Sem esta conversão o quadro sai da GPU perfeito e chega do
    /// outro lado como um NAL de tipo 0 que nenhum decodificador exibe.
    pub fn encode(
        &mut self,
        surface: &IOSurface,
        timestamp_ns: u64,
    ) -> Result<EncodedFrame, EncoderError> {
        if let Some(pacer) = self.pacer.as_mut()
            && !pacer.admit(timestamp_ns)
        {
            return Err(EncoderError::NeedsMoreInput);
        }

        // A hora de verdade da captura, em nanossegundos. Antes era um contador de
        // quadros sobre o fps nominal, e o VideoToolbox distribui o bitrate pelo relógio
        // que recebe: com a captura entregando 40 quadros por segundo e o contador
        // andando como se fossem 60, ele espalhava um segundo de bits por um segundo e
        // meio de vídeo real — a transmissão saía com dois terços do bitrate pedido.
        let presentation = (timestamp_ns as i64, 1_000_000_000);

        let encoded = self
            .session
            .encode(surface, presentation)
            .map_err(|error| EncoderError::Encode(error.to_string()))?;

        let format = encoded
            .cm_sample_buffer()
            .and_then(|buffer| buffer.format_description());
        let header_len = format.as_ref().map_or(4, nal_header_length);
        let (bitstream, keyframe) = to_annex_b(&encoded.data, header_len);

        if !keyframe {
            return Ok(EncodedFrame {
                data: bitstream,
                keyframe,
                timestamp_ns,
            });
        }

        // Falhar alto de propósito: um keyframe sem SPS/PPS produz uma transmissão que
        // parece saudável em todo contador e não abre em nenhuma tela.
        let mut data = format
            .as_ref()
            .and_then(parameter_sets)
            .ok_or_else(|| EncoderError::Encode("keyframe sem SPS/PPS".into()))?;

        data.extend_from_slice(&bitstream);

        Ok(EncodedFrame {
            data,
            keyframe,
            timestamp_ns,
        })
    }
}

/// Quantos bytes o prefixo de tamanho do AVCC ocupa. O VideoToolbox usa 4, mas o valor
/// vem da descrição em vez de ser assumido.
fn nal_header_length(format: &CMFormatDescription) -> usize {
    let mut length = 0i32;

    let status = unsafe {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format.as_ptr(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut length,
        )
    };

    if status == 0 && (1..=4).contains(&length) {
        length as usize
    } else {
        4
    }
}

/// SPS e PPS em Annex-B, prontos para ir na frente de um keyframe.
fn parameter_sets(format: &CMFormatDescription) -> Option<Vec<u8>> {
    let mut total = 0usize;

    let status = unsafe {
        CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            format.as_ptr(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut total,
            ptr::null_mut(),
        )
    };

    if status != 0 || total == 0 {
        return None;
    }

    let mut output = Vec::new();

    for index in 0..total {
        let mut data: *const u8 = ptr::null();
        let mut size = 0usize;

        let status = unsafe {
            CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                format.as_ptr(),
                index,
                &mut data,
                &mut size,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        if status != 0 || data.is_null() || size == 0 {
            return None;
        }

        output.extend_from_slice(&START_CODE);
        // Os bytes pertencem à descrição de formato, que continua viva neste escopo.
        output.extend_from_slice(unsafe { slice::from_raw_parts(data, size) });
    }

    Some(output)
}

/// AVCC para Annex-B. Devolve também se o quadro carrega um IDR, que é o que decide se
/// SPS e PPS precisam ir junto.
fn to_annex_b(avcc: &[u8], header_len: usize) -> (Vec<u8>, bool) {
    let mut output = Vec::with_capacity(avcc.len() + 16);
    let mut is_idr = false;
    let mut position = 0;

    while position + header_len <= avcc.len() {
        let size = avcc[position..position + header_len]
            .iter()
            .fold(0usize, |total, byte| (total << 8) | *byte as usize);

        position += header_len;

        let end = position.saturating_add(size);

        // Tamanho zero ou estourando o buffer significa que o prefixo lido está errado.
        // Parar é melhor do que caminhar para dentro do lixo.
        if size == 0 || end > avcc.len() {
            break;
        }

        is_idr |= avcc[position] & 0x1F == 5;
        output.extend_from_slice(&START_CODE);
        output.extend_from_slice(&avcc[position..end]);
        position = end;
    }

    (output, is_idr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avcc_becomes_annex_b_and_finds_the_idr() {
        // Dois NALs: um SEI (tipo 6) e um IDR (tipo 5).
        let avcc = [0, 0, 0, 2, 0x06, 0xAA, 0, 0, 0, 3, 0x65, 0xBB, 0xCC];

        let (output, is_idr) = to_annex_b(&avcc, 4);

        assert!(
            is_idr,
            "o IDR precisa ser reconhecido, senão o keyframe vai sem SPS/PPS"
        );
        assert_eq!(
            output,
            [0, 0, 0, 1, 0x06, 0xAA, 0, 0, 0, 1, 0x65, 0xBB, 0xCC],
        );
    }

    #[test]
    fn frame_without_idr_does_not_ask_for_parameter_sets() {
        let avcc = [0, 0, 0, 2, 0x41, 0xAA];

        let (output, is_idr) = to_annex_b(&avcc, 4);

        assert!(!is_idr);
        assert_eq!(output, [0, 0, 0, 1, 0x41, 0xAA]);
    }

    #[test]
    fn lying_prefix_stops_instead_of_walking_into_garbage() {
        // Diz que o NAL tem 99 bytes num buffer que tem 2.
        let avcc = [0, 0, 0, 99, 0x65, 0xAA];

        let (output, is_idr) = to_annex_b(&avcc, 4);

        assert!(output.is_empty());
        assert!(!is_idr);
    }
}
