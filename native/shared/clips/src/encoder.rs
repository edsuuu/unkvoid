//! Encoder de H.264 por hardware: Media Foundation por cima da placa de vídeo (NVENC,
//! QuickSync ou VCE, conforme a máquina).
//!
//! Adaptado de `unkvoid/native/shared/media/src/windows.rs`. O caminho é o mesmo — a
//! textura da captura atravessa para um device com suporte a vídeo, o VideoProcessor
//! converte BGRA em NV12 e o MFT da placa comprime, sem o quadro passar pela CPU — com o
//! que muda entre transmitir e gravar:
//!
//! - taxa de arquivo, não de rede, e perfil High;
//! - o tempo de cada quadro sai da amostra que o MFT devolve. O unkvoid carimba a saída com
//!   o tempo do quadro que acabou de entrar; com a fila do encoder no meio, a imagem ficaria
//!   alguns quadros deslocada do som;
//! - um anel de texturas NV12: o MFT é assíncrono e pode estar lendo a do quadro anterior
//!   quando o blit seguinte começa;
//! - sem o degrau do processador: sem encoder na placa, gravar a tela inteira na CPU tira do
//!   jogo exatamente o que o app promete não tirar. Melhor avisar que não dá.

use std::collections::VecDeque;
use std::sync::Once;

use anyhow::{Context, anyhow, bail};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_COLOR_SPACE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE, D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_0_255,
    D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_QUALITY,
    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11CreateDevice,
    ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D, ID3D11VideoContext,
    ID3D11VideoContext1, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{IDXGIKeyedMutex, IDXGIResource};
use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonMaxBitRate, CODECAPI_AVEncCommonMeanBitRate,
    CODECAPI_AVEncCommonQualityVsSpeed, CODECAPI_AVEncCommonRateControlMode,
    CODECAPI_AVEncMPVDefaultBPictureCount, CODECAPI_AVEncMPVGOPSize, ICodecAPI, IMFActivate,
    IMFDXGIDeviceManager, IMFMediaEventGenerator, IMFMediaType, IMFSample, IMFTransform,
    METransformHaveOutput, METransformNeedInput, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_EVENT_TYPE,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE,
    MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
    MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION, MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer,
    MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video,
    MFNominalRange_16_235, MFSTARTUP_NOSOCKET, MFStartup, MFT_CATEGORY_VIDEO_ENCODER,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_FRIENDLY_NAME_Attribute,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFVideoPrimaries_BT709, MFVideoTransFunc_709,
    MFVideoTransferMatrix_BT709, eAVEncCommonRateControlMode_PeakConstrainedVBR,
    eAVEncH264VProfile_High,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_UI4};
use windows::core::{GUID, Interface, PWSTR};

/// A unidade de tempo do Media Foundation: 100 nanossegundos.
const HUNDRED_NANOSECONDS_PER_SECOND: i64 = 10_000_000;

/// Segundos entre quadros-chave. O clipe só pode começar num quadro-chave, então isto é a
/// precisão do começo do replay; mais curto engorda o arquivo, porque o quadro-chave é o
/// mais caro.
const KEYFRAME_SECONDS: u32 = 2;

/// Quantas texturas NV12 giram entre o blit e o MFT. O MFT da NVIDIA segura um ou dois
/// quadros na fila; oito é folga para qualquer placa, e custa 24 MB de memória de vídeo em
/// 1080p.
const NV12_RING: usize = 8;

/// Prazo para tomar cada lado do keyed mutex, em milissegundos. Esperar para sempre por uma
/// chave que o outro lado não vai devolver — driver que reiniciou — penduraria a captura
/// sem erro e sem log.
const LOCK_TIMEOUT_MS: u32 = 1_000;

static MEDIA_FOUNDATION: Once = Once::new();

#[derive(Clone, Copy, Debug)]
pub struct EncoderSettings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: u32,
    pub bitrate: u32,
}

pub struct EncodedFrame {
    /// Annex-B, com SPS e PPS na frente de cada quadro-chave.
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub timestamp_ns: u64,
}

pub struct Encoder {
    device: ID3D11Device,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,

    /// Guardado só para continuar existindo: o MFT recebe o gerente como número cru e não é
    /// dono dele. Solto, o encoder ficava com um ponteiro para nada no primeiro quadro.
    _manager: IMFDXGIDeviceManager,
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    settings: EncoderSettings,
    bridge: Option<Bridge>,
    pacer: FramePacer,
    ready: VecDeque<EncodedFrame>,

    /// Pedidos de entrada que o MFT já fez e ainda não foram atendidos. Um MFT assíncrono
    /// não repete um `METransformNeedInput` ignorado; sem guardar o pedido ele para de pedir
    /// e o `GetEvent`, que bloqueia sem prazo, pendura a captura (medido no unkvoid).
    credits: u32,
}

/// O encoder nasce na thread da captura e morre nela; o `Send` existe só para a
/// `windows-capture` poder guardá-lo no handler. O device é criado sem `SINGLETHREADED` e com
/// a proteção multithread ligada, e o MFT assíncrono é livre por contrato.
unsafe impl Send for Encoder {}

/// O caminho do device da captura para o do encoder, montado por tamanho de origem.
///
/// São dois devices porque o da captura nasce sem suporte a vídeo (a crate não expõe as
/// flags), e sem ele não há VideoProcessor nem gerente para o Media Foundation.
struct Bridge {
    source: (u32, u32),
    /// O contexto da captura que a ponte atende: a duplicação do Windows 10 volta com um
    /// device novo depois de cair, e a cópia para a textura do antigo não chegaria aqui.
    capture: ID3D11DeviceContext,
    shared_with_capture: ID3D11Texture2D,
    capture_lock: IDXGIKeyedMutex,
    my_lock: IDXGIKeyedMutex,
    processor: ID3D11VideoProcessor,
    input: ID3D11VideoProcessorInputView,
    targets: Vec<(ID3D11Texture2D, ID3D11VideoProcessorOutputView)>,
    next_target: usize,
}

impl Encoder {
    pub fn new(settings: EncoderSettings) -> anyhow::Result<Self> {
        unsafe {
            start_media_foundation()?;

            let (device, context) = create_device()?;
            let multithread: ID3D11Multithread = device.cast()?;

            let _ = multithread.SetMultithreadProtected(true);

            let video_device: ID3D11VideoDevice = device.cast()?;
            let video_context: ID3D11VideoContext = context.cast()?;

            // O gerente é como o MFT descobre em qual placa a textura vive. Sem ele o encoder
            // recusa qualquer amostra que não esteja na memória do processador.
            let mut token = 0_u32;
            let mut manager: Option<IMFDXGIDeviceManager> = None;

            MFCreateDXGIDeviceManager(&mut token, &mut manager)?;

            let manager = manager.context("o Media Foundation não devolveu o gerente de device")?;

            manager.ResetDevice(&device, token)?;

            let transform = open_encoder(&manager, &settings)?;
            let events = transform.cast()?;

            Ok(Self {
                device,
                video_device,
                video_context,
                _manager: manager,
                transform,
                events,
                settings,
                bridge: None,
                pacer: FramePacer::new(settings.frame_rate),
                ready: VecDeque::new(),
                credits: 0,
            })
        }
    }

    /// Codifica um quadro que ainda está na GPU e devolve o que o MFT já tiver pronto — em
    /// geral um quadro, nenhum enquanto a fila do encoder enche.
    ///
    /// A textura é da rotação interna da captura e só vale durante o callback: a cópia para
    /// a ponte acontece aqui dentro, antes de devolver.
    pub fn encode(
        &mut self,
        texture: &ID3D11Texture2D,
        context: &ID3D11DeviceContext,
        timestamp_ns: u64,
    ) -> anyhow::Result<Vec<EncodedFrame>> {
        // Antes da ponte: o quadro acima do teto não custa nem o blit. No Windows sem o
        // intervalo mínimo da captura (anterior ao 11 24H2) ela chega na frequência do
        // monitor, e um de 240 Hz encheria a placa com quatro vezes os quadros.
        if !self.pacer.admit(timestamp_ns) {
            return Ok(Vec::new());
        }

        unsafe {
            let target = self.cross_the_bridge(texture, context)?;
            let sample = self.build_sample(&target, timestamp_ns)?;

            self.pump(sample)?;
        }

        Ok(self.ready.drain(..).collect())
    }

    /// Leva o quadro do device da captura para este, já em NV12 e no tamanho de saída, e
    /// devolve a textura do anel que recebeu o blit.
    unsafe fn cross_the_bridge(
        &mut self,
        texture: &ID3D11Texture2D,
        context: &ID3D11DeviceContext,
    ) -> anyhow::Result<ID3D11Texture2D> {
        let mut description = D3D11_TEXTURE2D_DESC::default();

        unsafe { texture.GetDesc(&mut description) };

        let source = (description.Width, description.Height);

        if self.bridge.as_ref().is_none_or(|bridge| bridge.source != source || bridge.capture != *context) {
            self.bridge = Some(unsafe { self.build_bridge(texture, context, source)? });
        }

        let bridge = self.bridge.as_mut().expect("acabou de ser montada");
        let (target, output) = bridge.targets[bridge.next_target].clone();

        bridge.next_target = (bridge.next_target + 1) % bridge.targets.len();

        unsafe {
            // Chave 0 é o lado da captura, chave 1 é o meu: é a alternância que garante que
            // a cópia terminou antes de o blit começar.
            bridge.capture_lock.AcquireSync(0, LOCK_TIMEOUT_MS)?;
            context.CopyResource(&bridge.shared_with_capture, texture);

            // A cópia é assíncrona na GPU: soltar a chave antes do Flush deixava o encoder
            // ler a textura pela metade, e o vídeo saía preto (medido no unkvoid).
            context.Flush();
            bridge.capture_lock.ReleaseSync(1)?;
            bridge.my_lock.AcquireSync(1, LOCK_TIMEOUT_MS)?;

            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(bridge.input.clone())),
                ..Default::default()
            };

            let blit =
                self.video_context.VideoProcessorBlt(&bridge.processor, &output, 0, &[stream]);

            // A chave volta antes do erro subir: saindo com ela na mão, o quadro seguinte
            // esperaria por ela para sempre.
            bridge.my_lock.ReleaseSync(0)?;
            blit?;
        }

        Ok(target)
    }

    unsafe fn build_bridge(
        &self,
        texture: &ID3D11Texture2D,
        context: &ID3D11DeviceContext,
        source: (u32, u32),
    ) -> anyhow::Result<Bridge> {
        tracing::info!(width = source.0, height = source.1, "encoder: montando a ponte entre os devices");

        unsafe {
            let capture_device = texture.GetDevice()?;
            let description = D3D11_TEXTURE2D_DESC {
                Width: source.0,
                Height: source.1,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0 as u32,
            };

            let mut shared: Option<ID3D11Texture2D> = None;

            capture_device.CreateTexture2D(&description, None, Some(&mut shared))?;

            let shared = shared.context("a textura compartilhada não foi criada")?;
            let resource: IDXGIResource = shared.cast()?;
            let handle: HANDLE = resource.GetSharedHandle()?;
            let mut mine: Option<ID3D11Texture2D> = None;

            self.device.OpenSharedResource(handle, &mut mine)?;

            let mine = mine.context("a textura compartilhada não abriu no device do encoder")?;
            let (processor, enumerator) = self.create_processor(source)?;
            let mut input: Option<ID3D11VideoProcessorInputView> = None;

            self.video_device.CreateVideoProcessorInputView(
                &mine,
                &enumerator,
                &D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                    FourCC: 0,
                    ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                    ..Default::default()
                },
                Some(&mut input),
            )?;

            let mut targets = Vec::with_capacity(NV12_RING);

            for _ in 0..NV12_RING {
                let nv12 = nv12_texture(&self.device, self.settings.width, self.settings.height)?;
                let mut output: Option<ID3D11VideoProcessorOutputView> = None;

                self.video_device.CreateVideoProcessorOutputView(
                    &nv12,
                    &enumerator,
                    &D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                        ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                        ..Default::default()
                    },
                    Some(&mut output),
                )?;

                targets.push((nv12, output.context("a view de saída não foi criada")?));
            }

            Ok(Bridge {
                source,
                capture: context.clone(),
                capture_lock: shared.cast()?,
                my_lock: mine.cast()?,
                shared_with_capture: shared,
                processor,
                input: input.context("a view de entrada não foi criada")?,
                targets,
                next_target: 0,
            })
        }
    }

    unsafe fn create_processor(
        &self,
        source: (u32, u32),
    ) -> anyhow::Result<(ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator)> {
        unsafe {
            let rate = DXGI_RATIONAL { Numerator: self.settings.frame_rate, Denominator: 1 };
            let enumerator = self.video_device.CreateVideoProcessorEnumerator(
                &D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                    InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                    InputFrameRate: rate,
                    InputWidth: source.0,
                    InputHeight: source.1,
                    OutputFrameRate: rate,
                    OutputWidth: self.settings.width,
                    OutputHeight: self.settings.height,
                    // O filtro de escala melhor é o que separa texto legível de borrado.
                    Usage: D3D11_VIDEO_USAGE_OPTIMAL_QUALITY,
                },
            )?;
            let processor = self.video_device.CreateVideoProcessor(&enumerator, 0)?;

            // Sem processamento automático o driver não aplica realce nem redução de ruído
            // por conta própria: a tela sai como está.
            self.video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);

            // Entra RGB cheio (0–255), sai NV12 BT.709 limitado (16–235), o que todo
            // decodificador assume. Com tudo zerado o driver convertia em BT.601 e a imagem
            // saía escura e lavada (medido no unkvoid). A interface nova diz isso sem
            // ambiguidade; a antiga, campo de bits, fica para driver que não tem a nova.
            match self.video_context.cast::<ID3D11VideoContext1>() {
                Ok(context) => {
                    context.VideoProcessorSetStreamColorSpace1(
                        &processor,
                        0,
                        DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709,
                    );
                    context.VideoProcessorSetOutputColorSpace1(
                        &processor,
                        DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                    );
                }
                Err(_) => {
                    self.video_context.VideoProcessorSetStreamColorSpace(
                        &processor,
                        0,
                        &color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_0_255),
                    );
                    self.video_context.VideoProcessorSetOutputColorSpace(
                        &processor,
                        &color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235),
                    );
                }
            }

            Ok((processor, enumerator))
        }
    }

    unsafe fn build_sample(
        &self,
        target: &ID3D11Texture2D,
        timestamp_ns: u64,
    ) -> anyhow::Result<IMFSample> {
        unsafe {
            let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, target, 0, false)?;
            let sample = MFCreateSample()?;

            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime((timestamp_ns / 100) as i64)?;
            sample.SetSampleDuration(
                HUNDRED_NANOSECONDS_PER_SECOND / i64::from(self.settings.frame_rate),
            )?;

            Ok(sample)
        }
    }

    /// Roda a fila de eventos do MFT até entregar o quadro e ter uma saída, ou até ele pedir
    /// entrada sem haver mais nenhuma — o que acontece enquanto a fila dele enche.
    unsafe fn pump(&mut self, sample: IMFSample) -> anyhow::Result<()> {
        unsafe {
            let mut input = Some(sample);

            if self.credits > 0
                && let Some(sample) = input.take()
            {
                self.transform.ProcessInput(0, &sample, 0)?;
                self.credits -= 1;
            }

            while input.is_some() || self.ready.is_empty() {
                let event = self.events.GetEvent(Default::default())?;
                let kind = MF_EVENT_TYPE(event.GetType()? as i32);

                if kind == METransformNeedInput {
                    let Some(sample) = input.take() else {
                        self.credits += 1;

                        return Ok(());
                    };

                    self.transform.ProcessInput(0, &sample, 0)?;
                } else if kind == METransformHaveOutput {
                    self.collect_output()?;
                }
            }

            Ok(())
        }
    }

    unsafe fn collect_output(&mut self) -> anyhow::Result<()> {
        unsafe {
            let mut output = [MFT_OUTPUT_DATA_BUFFER::default()];
            let mut status = 0_u32;
            let info = self.transform.GetOutputStreamInfo(0)?;

            if info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 == 0 {
                let size = info.cbSize.max(self.settings.width * self.settings.height * 3 / 2);
                let sample = MFCreateSample()?;

                sample.AddBuffer(&MFCreateMemoryBuffer(size)?)?;
                output[0].pSample = std::mem::ManuallyDrop::new(Some(sample));
            }

            let result = self.transform.ProcessOutput(0, &mut output, &mut status);
            let sample = output[0].pSample.take();

            match result {
                Ok(()) => {}
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                Err(error) => return Err(error.into()),
            }

            let sample = sample.context("o encoder não devolveu amostra")?;
            let timestamp_ns = crate::clock::from_hundred_nanoseconds(sample.GetSampleTime()?);
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut start = std::ptr::null_mut();
            let mut size = 0_u32;

            buffer.Lock(&mut start, None, Some(&mut size))?;

            let data = std::slice::from_raw_parts(start, size as usize).to_vec();

            buffer.Unlock()?;

            self.ready.push_back(EncodedFrame { keyframe: is_keyframe(&data), data, timestamp_ns });

            Ok(())
        }
    }
}

/// Descarta quadros acima do teto de fps antes de qualquer trabalho de GPU.
struct FramePacer {
    interval_ns: u64,
    next_ns: u64,
}

impl FramePacer {
    fn new(frame_rate: u32) -> Self {
        Self { interval_ns: 1_000_000_000 / u64::from(frame_rate.max(1)), next_ns: 0 }
    }

    fn admit(&mut self, timestamp_ns: u64) -> bool {
        // Um quarto de quadro de folga: a 60 Hz a captura não chega a cada 16 666 µs exatos,
        // e sem folga o quadro que devia passar chegava um tico adiantado e ficava de fora
        // (medido no unkvoid: caía para 20 fps).
        if timestamp_ns + self.interval_ns / 4 < self.next_ns {
            return false;
        }

        let base = if timestamp_ns > self.next_ns + self.interval_ns { timestamp_ns } else { self.next_ns };

        self.next_ns = base + self.interval_ns;

        true
    }
}

/// Controle de taxa para arquivo: VBR com pico, qualidade acima de velocidade, sem quadros B
/// (o MP4 fica sem reordenação de tempo) e um quadro-chave a cada `KEYFRAME_SECONDS`.
///
/// Nada aqui é fatal: o MFT recusa o que não implementa, e gravar com o padrão do driver
/// ainda é gravar. O recusado vai para o log.
unsafe fn tune(transform: &IMFTransform, settings: &EncoderSettings) {
    let Ok(codec) = transform.cast::<ICodecAPI>() else {
        tracing::warn!("encoder: sem ICodecAPI, o controle de taxa fica no padrão do driver");

        return;
    };

    let values: [(&GUID, u32, &str); 6] = [
        (
            &CODECAPI_AVEncCommonRateControlMode,
            eAVEncCommonRateControlMode_PeakConstrainedVBR.0 as u32,
            "modo de taxa",
        ),
        (&CODECAPI_AVEncCommonMeanBitRate, settings.bitrate, "taxa média"),
        (&CODECAPI_AVEncCommonMaxBitRate, settings.bitrate / 2 * 3, "taxa de pico"),
        (&CODECAPI_AVEncCommonQualityVsSpeed, 100, "qualidade sobre velocidade"),
        (&CODECAPI_AVEncMPVDefaultBPictureCount, 0, "sem quadros B"),
        (&CODECAPI_AVEncMPVGOPSize, settings.frame_rate * KEYFRAME_SECONDS, "intervalo de quadro-chave"),
    ];

    for (key, value, name) in values {
        if let Err(error) = unsafe { codec.SetValue(key, &unsigned_variant(value)) } {
            tracing::warn!(error = %error, setting = name, "encoder: ajuste recusado");
        }
    }
}

/// `VARIANT` do tipo `VT_UI4`, montado campo a campo: o `From<u32>` da crate monta outro
/// tipo, e o `ICodecAPI` recusa.
fn unsigned_variant(value: u32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_UI4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { ulVal: value },
            }),
        },
    }
}

pub fn start_media_foundation() -> anyhow::Result<()> {
    let mut failure = None;

    MEDIA_FOUNDATION.call_once(|| {
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) } {
            failure = Some(error);
        }
    });

    match failure {
        Some(error) => Err(anyhow!(error).context("MFStartup")),
        None => Ok(()),
    }
}

/// Um device só para codificar, com suporte a vídeo. O da captura nasce sem a flag; um
/// device a mais na mesma placa custa memória, não desempenho.
unsafe fn create_device() -> anyhow::Result<(ID3D11Device, ID3D11DeviceContext)> {
    unsafe {
        let mut device = None;
        let mut context = None;

        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .context("D3D11CreateDevice")?;

        Ok((
            device.context("o Direct3D não devolveu o device")?,
            context.context("o Direct3D não devolveu o contexto")?,
        ))
    }
}

/// O primeiro MFT de H.264 da placa que aceitar a configuração inteira. O primeiro da lista
/// não basta: num notebook com duas placas o Windows pode anunciar o da NVIDIA na frente com
/// o device na Intel.
unsafe fn open_encoder(
    manager: &IMFDXGIDeviceManager,
    settings: &EncoderSettings,
) -> anyhow::Result<IMFTransform> {
    unsafe {
        let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_NV12 };
        let output = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_H264 };
        let mut found: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0_u32;

        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut found,
            &mut count,
        )?;

        let candidates: Vec<IMFActivate> = if found.is_null() {
            Vec::new()
        } else {
            let taken = std::slice::from_raw_parts_mut(found, count as usize)
                .iter_mut()
                .filter_map(Option::take)
                .collect();

            CoTaskMemFree(Some(found.cast::<core::ffi::c_void>().cast_const()));

            taken
        };

        for activate in candidates {
            let name = friendly_name(&activate);

            match try_encoder(&activate, manager, settings) {
                Ok(transform) => {
                    tracing::info!(name = %name, ?settings, "encoder: MFT escolhido");

                    return Ok(transform);
                }
                Err(error) => {
                    tracing::warn!(error = %error, name = %name, "encoder: MFT recusou, tentando o próximo");

                    let _ = activate.ShutdownObject();
                }
            }
        }

        bail!("esta máquina não tem encoder de H.264 na placa de vídeo")
    }
}

unsafe fn try_encoder(
    activate: &IMFActivate,
    manager: &IMFDXGIDeviceManager,
    settings: &EncoderSettings,
) -> anyhow::Result<IMFTransform> {
    unsafe {
        let transform: IMFTransform = activate.ActivateObject()?;

        // Encoder de hardware nasce trancado: sem destrancar, ele recusa ProcessInput.
        transform.GetAttributes()?.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
        transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;
        configure_types(&transform, settings)?;

        // O MFT da placa lê o controle de taxa depois dos tipos (medido no unkvoid).
        tune(&transform, settings);

        transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
        transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

        Ok(transform)
    }
}

unsafe fn friendly_name(activate: &IMFActivate) -> String {
    unsafe {
        let mut name = PWSTR::null();
        let mut length = 0_u32;

        if activate.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut length).is_err() {
            return "sem nome".into();
        }

        let text = name.to_string().unwrap_or_default();

        CoTaskMemFree(Some(name.0.cast::<core::ffi::c_void>().cast_const()));

        text
    }
}

unsafe fn nv12_texture(device: &ID3D11Device, width: u32, height: u32) -> anyhow::Result<ID3D11Texture2D> {
    unsafe {
        let mut texture: Option<ID3D11Texture2D> = None;

        device.CreateTexture2D(
            &D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            },
            None,
            Some(&mut texture),
        )?;

        texture.context("a textura NV12 não foi criada")
    }
}

/// A saída primeiro, a entrada depois: o MFT recusa a entrada enquanto não souber o que tem
/// de produzir. O tipo de saída tenta com cor e perfil High; driver que recusa um dos dois
/// grava com o padrão dele.
unsafe fn configure_types(transform: &IMFTransform, settings: &EncoderSettings) -> anyhow::Result<()> {
    unsafe {
        let output_type = |complete: bool| -> anyhow::Result<IMFMediaType> {
            let output: IMFMediaType = MFCreateMediaType()?;

            output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, settings.bitrate)?;
            output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            output.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 0)?;
            set_pair(&output, &MF_MT_FRAME_SIZE, settings.width, settings.height)?;
            set_pair(&output, &MF_MT_FRAME_RATE, settings.frame_rate, 1)?;

            if complete {
                // High em vez do Main padrão: a transformada 8×8 é o que segura texto e bordas
                // finas na mesma taxa.
                output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
                set_color(&output)?;
            }

            Ok(output)
        };

        if let Err(error) = transform.SetOutputType(0, Some(&output_type(true)?), 0) {
            tracing::warn!(error = %error, "encoder: tipo de saída completo recusado, tentando o básico");

            transform.SetOutputType(0, Some(&output_type(false)?), 0)?;
        }

        let input: IMFMediaType = MFCreateMediaType()?;

        input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        set_pair(&input, &MF_MT_FRAME_SIZE, settings.width, settings.height)?;
        set_pair(&input, &MF_MT_FRAME_RATE, settings.frame_rate, 1)?;
        set_color(&input)?;
        transform.SetInputType(0, Some(&input), 0)?;

        Ok(())
    }
}

/// O mesmo espaço de cor que o processador produz, para o encoder escrever o VUI no SPS e o
/// player não chutar matriz nem faixa.
unsafe fn set_color(kind: &IMFMediaType) -> anyhow::Result<()> {
    unsafe {
        kind.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
        kind.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
        kind.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
        kind.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?;

        Ok(())
    }
}

/// `D3D11_VIDEO_PROCESSOR_COLOR_SPACE` é um campo de bits: matriz BT.709 no bit 2 e a faixa
/// nominal nos bits 4–5. Só a faixa muda entre entrada e saída.
fn color_space(range: D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE) -> D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
    const MATRIX_BT709: u32 = 1 << 2;

    D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: MATRIX_BT709 | ((range.0 as u32) << 4) }
}

/// Tamanho e taxa de quadros moram num atributo só, empacotados em 64 bits.
unsafe fn set_pair(kind: &IMFMediaType, key: &GUID, high: u32, low: u32) -> anyhow::Result<()> {
    unsafe { Ok(kind.SetUINT64(key, (u64::from(high) << 32) | u64::from(low))?) }
}

/// Um quadro-chave de H.264 traz IDR (NAL 5), com SPS (7) e PPS (8) na frente.
fn is_keyframe(data: &[u8]) -> bool {
    crate::clip::nal_units(data).any(|unit| matches!(unit.first().map(|byte| byte & 0x1F), Some(5 | 7 | 8)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_space_packs_the_bitfield_like_d3d11_h() {
        assert_eq!(color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_0_255)._bitfield, 0b10_0100);
        assert_eq!(color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235)._bitfield, 0b01_0100);
    }

    #[test]
    fn pacer_keeps_sixty_out_of_two_hundred_forty() {
        let mut pacer = FramePacer::new(60);
        let admitted = (0..240_u64).filter(|frame| pacer.admit(frame * 1_000_000_000 / 240)).count();

        assert!((59..=61).contains(&admitted), "{admitted}");
    }
}
