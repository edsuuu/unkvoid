//! Encoder de hardware no Windows: Media Foundation por cima da placa de vídeo.
//!
//! O quadro chega como textura do Direct3D, já na GPU, e sai como H.264 sem nunca
//! passar pela CPU — que é o ponto do app inteiro: comprimir 1080p60 no processador
//! rouba do jogo exatamente o que ele precisa.
//!
//! O MFT de H.264 por hardware é o mesmo caminho que o NVENC (NVIDIA), o QuickSync
//! (Intel) e o VCE (AMD) expõem ao Windows. Sem nenhum que aceite, cai para o MFT de
//! software do próprio Windows em 720p30 — a única vez em que o quadro desce para a CPU
//! (`Backend::Cpu`).

use std::collections::VecDeque;
use std::sync::Once;

use ::windows::Win32::Foundation::{HANDLE, VARIANT_BOOL};
use ::windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_1};
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_COLOR_SPACE,
    D3D11_VIDEO_PROCESSOR_CONTENT_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE, D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_0_255,
    D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_QUALITY, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11Multithread, ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoContext1, ID3D11VideoDevice,
    ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView,
};
use ::windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use ::windows::Win32::Graphics::Dxgi::{IDXGIKeyedMutex, IDXGIResource};
use ::windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncCommonLowLatency, CODECAPI_AVEncCommonMaxBitRate, CODECAPI_AVEncCommonMeanBitRate,
    CODECAPI_AVEncCommonQualityVsSpeed, CODECAPI_AVEncCommonRateControlMode,
    CODECAPI_AVEncCommonRealTime, CODECAPI_AVEncMPVDefaultBPictureCount, CODECAPI_AVEncMPVGOPSize,
    CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode,
    ICodecAPI, IMFActivate, IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaEventGenerator,
    IMFMediaType, IMFSample, IMFTransform, METransformHaveOutput, METransformNeedInput,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_EVENT_TYPE, MF_MT_ALL_SAMPLES_INDEPENDENT,
    MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
    MF_MT_YUV_MATRIX, MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION, MFCreateDXGIDeviceManager,
    MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFMediaType_Video, MFSTARTUP_NOSOCKET, MFStartup, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_FRIENDLY_NAME_Attribute, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MFTEnumEx, MFNominalRange_16_235,
    MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFVideoPrimaries_BT709,
    MFVideoTransFunc_709, MFVideoTransferMatrix_BT709,
    eAVEncCommonRateControlMode_PeakConstrainedVBR,
};
use ::windows::Win32::System::Com::CoTaskMemFree;
use ::windows::Win32::System::Variant::{
    VARENUM, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_UI4,
};
use ::windows::core::{Interface, PWSTR};

use crate::{EncodedFrame, EncoderConfig, EncoderError, FramePacer, GpuSurface, cpu_forced};

/// A unidade de tempo do Media Foundation: 100 nanossegundos.
const HNS_PER_SECOND: i64 = 10_000_000;

/// `MFStartup` é por processo, e chamar duas vezes devolve erro.
static MF_STARTUP: Once = Once::new();

/// Prazo para tomar cada lado do keyed mutex, em milissegundos. Um quadro a 60 Hz dura
/// 16 ms; um segundo é folga de sobra. Esperar `INFINITE` por uma chave que o outro lado
/// não vai devolver — driver que reiniciou, ponte remontada no meio — penduraria a thread
/// da captura para sempre, sem erro e sem log.
const LOCK_TIMEOUT_MS: u32 = 1_000;

pub struct MediaFoundationEncoder {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,

    /// Guardado só para continuar existindo: o MFT recebe o gerente como número cru no
    /// `MFT_MESSAGE_SET_D3D_MANAGER` e não é dono dele. Sendo local de `new`, ele morria
    /// ao fim da abertura e o encoder ficava com um ponteiro para nada — que só cobra no
    /// primeiro quadro, dentro do driver.
    _manager: IMFDXGIDeviceManager,
    transform: IMFTransform,
    backend: Backend,
    width: u32,
    height: u32,
    frame_rate: f64,

    /// A taxa média em vigor. Nasce como a da configuração que abriu — no degrau do
    /// processador, a de 720p30 e não a pedida — e muda com `set_bitrate`.
    bitrate: u32,

    /// O MFT recusou trocar a taxa de pico no ar: avisado uma vez, e só.
    peak_refused: bool,
    bridge: Option<Bridge>,

    /// O teto de fps que a captura do Windows 10 não impõe — ver `FramePacer`.
    pacer: FramePacer,

    /// Pedido de quadro-chave esperando a próxima amostra.
    ///
    /// Guardado em vez de aplicado na hora porque a propriedade vale para o quadro
    /// seguinte: ligá-la fora do caminho de codificação a gastaria num momento em que
    /// não há quadro nenhum para marcar.
    force_keyframe: bool,
    /// Quantos quadros já foram descritos no log. Descrever todos encheria o disco.
    described: u8,
    /// Um encoder de hardware tem fila: nem todo quadro que entra sai no mesmo instante.
    ready: VecDeque<EncodedFrame>,

    /// Pedidos de entrada que o MFT já fez e ainda não foram atendidos.
    ///
    /// Um MFT assíncrono **não** repete um `METransformNeedInput` que ninguém atendeu.
    /// Consumir o evento sem entregar amostra queima o pedido para sempre, e o encoder
    /// de placa costuma pedir mais de um logo no começo. Depois de alguns quadros ele
    /// parava de pedir, ninguém tinha mais o que entregar, e o `GetEvent` — que é
    /// bloqueante e sem prazo — pendurava a thread da captura para sempre, sem erro e
    /// sem log. Guardar o pedido é o que impede isso.
    credits: u32,
}

/// O encoder nasce na thread que liga a transmissão e passa a viver na thread da
/// captura — atravessa a fronteira uma vez, e depois disso ninguém mais toca nele de
/// fora. O Rust não sabe disso porque ponteiro COM não é `Send` por padrão, e a regra
/// existe para objetos presos a um apartamento (COM STA).
///
/// Nenhum destes está: o device do Direct3D é criado sem `SINGLETHREADED` e com a
/// proteção multithread ligada, e o MFT assíncrono do Media Foundation é livre por
/// contrato — é das filas de trabalho do próprio MF que ele normalmente é chamado. A
/// `windows-capture` faz o mesmo com os handles dela, pelo mesmo motivo.
unsafe impl Send for MediaFoundationEncoder {}

/// De onde sai o H.264. O resto — ponte, conversão e escala no VideoProcessor, tipos,
/// controle de taxa — é o mesmo para os dois; muda como a amostra é montada e bombeada.
enum Backend {
    /// O MFT da placa: lê a textura NV12 direto e fala por eventos, assíncrono.
    Gpu { events: IMFMediaEventGenerator },

    /// O MFT de H.264 por software do próprio Windows, para quando não há placa que
    /// codifique. Só aceita memória do processador, então a textura NV12 — já convertida
    /// e no tamanho final — desce por `staging`, e responde na mesma chamada. É o degrau
    /// que o dono pediu: transmitir pior em vez de recusar, com o teto de
    /// `EncoderConfig::for_cpu` e o limitador do encoder segurando os 30 fps.
    Cpu { staging: ID3D11Texture2D },
}

/// O caminho de um device para o outro, montado uma vez por tamanho de origem.
///
/// São dois devices porque o da captura nasce sem suporte a vídeo (a crate que a
/// implementa não expõe as flags), e sem isso não há VideoProcessor nem gerente de
/// device para o Media Foundation. A ponte é uma textura compartilhada: a captura copia
/// nela, este lado lê dela, e o keyed mutex ordena os dois.
struct Bridge {
    source: (u32, u32),
    shared_with_capture: ID3D11Texture2D,
    capture_lock: IDXGIKeyedMutex,
    my_lock: IDXGIKeyedMutex,
    processor: ID3D11VideoProcessor,
    input: ID3D11VideoProcessorInputView,
    output: ID3D11VideoProcessorOutputView,
    nv12: ID3D11Texture2D,
}

impl MediaFoundationEncoder {
    pub fn new(config: &EncoderConfig) -> Result<Self, EncoderError> {
        // Um passo por linha, anunciado antes de acontecer. Daqui para baixo é tudo COM
        // e Direct3D: driver velho, placa sem encoder de hardware ou sessão sem GPU não
        // devolvem erro — derrubam o processo. A última linha no arquivo é o passo que
        // matou.
        unsafe {
            tracing::info!("encoder: MFStartup");

            start_media_foundation()?;

            tracing::info!("encoder: criando o device do Direct3D 11");

            let (device, context) = create_device()?;

            let multithread: ID3D11Multithread = device.cast().map_err(start_error)?;
            let _ = multithread.SetMultithreadProtected(true);

            let video_device: ID3D11VideoDevice = device.cast().map_err(start_error)?;
            let video_context: ID3D11VideoContext = context.cast().map_err(start_error)?;

            // O gerente é como o MFT descobre em qual placa a textura vive. Sem ele, o
            // encoder recusa qualquer amostra que não esteja na memória do processador.
            tracing::info!("encoder: ligando o gerente de device do DXGI");

            let mut token = 0_u32;
            let mut manager: Option<IMFDXGIDeviceManager> = None;

            MFCreateDXGIDeviceManager(&mut token, &mut manager).map_err(start_error)?;

            let manager = manager.ok_or_else(|| {
                EncoderError::Start("o Media Foundation não devolveu o gerente de device".into())
            })?;

            manager
                .ResetDevice(&device, token)
                .map_err(start_error)?;

            tracing::info!("encoder: procurando o MFT de H.264 por hardware");

            let hardware = if cpu_forced() {
                Err(EncoderError::Start("UNKVOID_ENCODER=cpu".into()))
            } else {
                open_encoder(MFT_ENUM_FLAG_HARDWARE, Some(&manager), config)
            };

            let (transform, backend, config) = match hardware {
                Ok(transform) => {
                    let events = transform.cast().map_err(start_error)?;

                    (transform, Backend::Gpu { events }, config.clone())
                }
                Err(error) => {
                    let config = config.for_cpu();

                    tracing::warn!(
                        error = %error,
                        width = config.width,
                        height = config.height,
                        frame_rate = config.frame_rate,
                        "encoder: sem encoder na placa, codificando no processador"
                    );

                    let transform = open_encoder(MFT_ENUM_FLAG_SYNCMFT, None, &config)?;
                    let staging = nv12_texture(&device, config.width, config.height, true)?;

                    (
                        transform,
                        Backend::Cpu { staging },
                        config,
                    )
                }
            };

            tracing::info!("encoder: pronto");

            Ok(Self {
                device,
                context,
                video_device,
                video_context,
                _manager: manager,
                transform,
                backend,
                width: config.width,
                height: config.height,
                frame_rate: config.frame_rate,
                bitrate: config.bitrate,
                peak_refused: false,
                bridge: None,
                pacer: FramePacer::new(config.frame_rate),
                force_keyframe: false,
                described: 0,
                ready: VecDeque::new(),
                credits: 0,
            })
        }
    }

    /// O servidor pediu um quadro-chave: o próximo sai como tal.
    pub fn request_keyframe(&mut self) {
        self.force_keyframe = true;
    }

    /// Se o H.264 sai do MFT da placa.
    pub fn hardware(&self) -> bool {
        matches!(self.backend, Backend::Gpu { .. })
    }

    /// A taxa média em vigor, que ao abrir é o teto de quem a ajusta.
    pub fn bitrate(&self) -> u32 {
        self.bitrate
    }

    /// Troca a taxa com o encoder no ar, sem reabrir nada. Devolve se o MFT aceitou.
    ///
    /// Recusa não derruba nada, como no `mark_keyframe`: a transmissão segue na taxa que
    /// tinha, a recusa vai para o log, e quem chamou para de tentar — é assim que o aviso
    /// sai uma vez só. Quem decide é a taxa média; MFT que aceita a média e recusa o pico
    /// continua valendo, só com rajadas medidas pelo teto antigo.
    pub fn set_bitrate(&mut self, bitrate: u32) -> bool {
        let Ok(codec) = self.transform.cast::<ICodecAPI>() else {
            tracing::warn!("encoder: sem ICodecAPI, a taxa fica fixa");

            return false;
        };

        let mean = (&CODECAPI_AVEncCommonMeanBitRate, bitrate);
        let peak = (&CODECAPI_AVEncCommonMaxBitRate, bitrate.saturating_add(bitrate / 2));

        let order = if bitrate > self.bitrate { [peak, mean] } else { [mean, peak] };

        for (key, value) in order {
            let Err(error) = (unsafe { codec.SetValue(key, &variant(VT_UI4, VARIANT_0_0_0 { ulVal: value })) }) else {
                continue;
            };

            if *key == CODECAPI_AVEncCommonMeanBitRate {
                tracing::warn!(error = %error, bitrate, "encoder: o MFT não troca a taxa no ar, ela fica fixa");

                return false;
            }

            if !std::mem::replace(&mut self.peak_refused, true) {
                tracing::warn!(error = %error, "encoder: o MFT não troca a taxa de pico no ar (as próximas não avisam)");
            }
        }

        self.bitrate = bitrate;

        true
    }

    /// Codifica um quadro. `surface` vem da captura sem passar pela CPU.
    pub fn encode(
        &mut self,
        surface: &GpuSurface,
        timestamp_ns: u64,
    ) -> Result<EncodedFrame, EncoderError> {
        // Antes da ponte: o quadro acima do teto não custa nem o blit. Vale para os dois
        // caminhos, e não só para o do processador: no Windows 10 a captura chega na
        // frequência do monitor, e um monitor de 144 Hz enchia a placa de quadros com o
        // bitrate pensado para 60.
        if !self.pacer.admit(timestamp_ns) {
            return Err(EncoderError::NeedsMoreInput);
        }

        unsafe {
            self.cross_the_bridge(surface)?;

            if std::mem::take(&mut self.force_keyframe) {
                self.mark_keyframe();
            }

            let sample = self.build_sample(timestamp_ns)?;

            match self.backend {
                Backend::Gpu { .. } => self.pump(Some(sample))?,
                // Síncrono: entra o quadro e sai tudo o que o MFT já tiver pronto.
                Backend::Cpu { .. } => {
                    self.transform
                        .ProcessInput(0, &sample, 0)
                        .map_err(encode_error)?;

                    while self.collect_output()? {}
                }
            }
        }

        self.ready
            .pop_front()
            .map(|frame| EncodedFrame {
                timestamp_ns,
                ..frame
            })
            .ok_or(EncoderError::NeedsMoreInput)
    }

    /// Leva o quadro do device da captura para este, já convertido e no tamanho pedido.
    ///
    /// A conversão e a escala são um blit do VideoProcessor: a captura entrega BGRA no
    /// tamanho nativo do monitor, o encoder quer NV12 no tamanho escolhido, e fazer essa
    /// conta no processador devolveria o problema de fps que o app existe para resolver.
    unsafe fn cross_the_bridge(&mut self, surface: &GpuSurface) -> Result<(), EncoderError> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();

        unsafe { surface.texture.GetDesc(&mut desc) };

        let source = (desc.Width, desc.Height);

        if self
            .bridge
            .as_ref()
            .is_none_or(|bridge| bridge.source != source)
        {
            self.bridge = Some(unsafe { self.build_bridge(surface, source)? });
        }

        let bridge = self.bridge.as_ref().expect("acabou de ser montada");

        unsafe {
            // Chave 0 é o lado da captura, chave 1 é o meu: a trava alterna entre os dois
            // e é o que garante que a cópia terminou antes do blit começar.
            bridge
                .capture_lock
                .AcquireSync(0, LOCK_TIMEOUT_MS)
                .map_err(encode_error)?;

            surface
                .context
                .CopyResource(&bridge.shared_with_capture, &surface.texture);

            // A cópia é assíncrona na GPU. Liberar a mutex antes do Flush deixava o
            // encoder ler a textura compartilhada antes de a captura terminar de
            // preenchê-la, produzindo vídeo preto apesar de o preview estar correto.
            surface.context.Flush();

            bridge
                .capture_lock
                .ReleaseSync(1)
                .map_err(encode_error)?;

            bridge
                .my_lock
                .AcquireSync(1, LOCK_TIMEOUT_MS)
                .map_err(encode_error)?;

            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(bridge.input.clone())),
                ..Default::default()
            };

            let blit =
                self.video_context
                    .VideoProcessorBlt(&bridge.processor, &bridge.output, 0, &[stream]);

            // A trava volta antes do erro subir. Com o `?` no blit, um quadro recusado
            // saía daqui com a chave na mão e o quadro seguinte esperava por ela para
            // sempre — a transmissão congelava sem ninguém errar de novo.
            bridge.my_lock.ReleaseSync(0).map_err(encode_error)?;

            blit.map_err(encode_error)?;
        }

        Ok(())
    }

    unsafe fn build_bridge(
        &self,
        surface: &GpuSurface,
        source: (u32, u32),
    ) -> Result<Bridge, EncoderError> {
        // A ponte nasce no PRIMEIRO quadro, já na thread da captura — depois de o app
        // dizer que está transmitindo. Um crash aqui parece crash "ao transmitir", e sem
        // esta linha ninguém distingue disso de uma falha na abertura do encoder.
        tracing::info!(
            source_width = source.0,
            source_height = source.1,
            target_width = self.width,
            target_height = self.height,
            "encoder: montando a ponte entre os devices"
        );

        unsafe {
            let descriptor = D3D11_TEXTURE2D_DESC {
                Width: source.0,
                Height: source.1,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0 as u32,
            };

            let mut shared: Option<ID3D11Texture2D> = None;

            surface
                .device
                .CreateTexture2D(&descriptor, None, Some(&mut shared))
                .map_err(encode_error)?;

            let shared = shared.ok_or_else(|| {
                EncoderError::Encode("a textura compartilhada não foi criada".into())
            })?;

            let resource: IDXGIResource = shared.cast().map_err(encode_error)?;
            let handle: HANDLE = resource.GetSharedHandle().map_err(encode_error)?;

            let mut mine: Option<ID3D11Texture2D> = None;

            self.device
                .OpenSharedResource(handle, &mut mine)
                .map_err(encode_error)?;

            let mine = mine.ok_or_else(|| {
                EncoderError::Encode("a textura compartilhada não abriu neste device".into())
            })?;

            let capture_lock: IDXGIKeyedMutex = shared.cast().map_err(encode_error)?;
            let my_lock: IDXGIKeyedMutex = mine.cast().map_err(encode_error)?;

            let (processor, enumerator) = self.create_processor(source)?;
            let nv12 = self.create_nv12()?;

            let input_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };

            let mut input: Option<ID3D11VideoProcessorInputView> = None;

            self.video_device
                .CreateVideoProcessorInputView(
                    &mine,
                    &enumerator,
                    &input_desc,
                    Some(&mut input),
                )
                .map_err(encode_error)?;

            let output_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };

            let mut output: Option<ID3D11VideoProcessorOutputView> = None;

            self.video_device
                .CreateVideoProcessorOutputView(&nv12, &enumerator, &output_desc, Some(&mut output))
                .map_err(encode_error)?;

            Ok(Bridge {
                source,
                shared_with_capture: shared,
                capture_lock,
                my_lock,
                processor,
                input: input.ok_or_else(|| {
                    EncoderError::Encode("a view de entrada não foi criada".into())
                })?,
                output: output
                    .ok_or_else(|| EncoderError::Encode("a view de saída não foi criada".into()))?,
                nv12,
            })
        }
    }

    unsafe fn create_processor(
        &self,
        source: (u32, u32),
    ) -> Result<(ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator), EncoderError> {
        unsafe {
            let rate = DXGI_RATIONAL {
                Numerator: self.frame_rate.round() as u32,
                Denominator: 1,
            };

            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: rate,
                InputWidth: source.0,
                InputHeight: source.1,
                OutputFrameRate: rate,
                OutputWidth: self.width,
                OutputHeight: self.height,
                // Qualidade em vez de velocidade: é um blit por quadro, e o filtro de
                // escala melhor é o que separa texto legível de texto borrado.
                Usage: D3D11_VIDEO_USAGE_OPTIMAL_QUALITY,
            };

            let enumerator = self
                .video_device
                .CreateVideoProcessorEnumerator(&content)
                .map_err(encode_error)?;

            let processor = self
                .video_device
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(encode_error)?;

            // Sem "processamento automático" o driver não aplica realce, redução de
            // ruído ou o que mais achar bonito por conta própria: a tela sai como está.
            self.video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);

            // Sem isto o processador converte com tudo zerado: matriz BT.601 e faixa
            // "indefinida", que o driver resolve como quiser. O H.264 saía sem dizer o
            // que fez, o decodificador de quem assiste chutava BT.709 e faixa limitada, e
            // a imagem chegava escura e lavada. Entra RGB cheio (0–255), sai NV12 BT.709
            // limitado (16–235) — o que todo decodificador assume quando ninguém avisa.
            //
            // A interface nova (Windows 10) diz isso por um enum sem ambiguidade; a antiga
            // é um campo de bits que alguns drivers lêem de outro jeito. Fica a nova
            // quando existe, a antiga quando não.
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

                    tracing::info!("encoder: espaço de cor pelo ID3D11VideoContext1");
                }
                Err(error) => {
                    self.video_context.VideoProcessorSetStreamColorSpace(
                        &processor,
                        0,
                        &color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_0_255),
                    );
                    self.video_context.VideoProcessorSetOutputColorSpace(
                        &processor,
                        &color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235),
                    );

                    tracing::info!(error = %error, "encoder: espaço de cor pelo campo de bits antigo");
                }
            }

            Ok((processor, enumerator))
        }
    }

    unsafe fn create_nv12(&self) -> Result<ID3D11Texture2D, EncoderError> {
        unsafe { nv12_texture(&self.device, self.width, self.height, false) }
    }

    /// Desce o quadro para a memória do processador, o único lugar de onde o MFT de
    /// software lê.
    ///
    /// Só no degrau sem placa. A conversão e a escala continuam no VideoProcessor: o que
    /// atravessa é o NV12 já em 720p, 1,5 byte por pixel, e não o BGRA do monitor inteiro.
    unsafe fn read_back(
        &self,
        nv12: &ID3D11Texture2D,
        staging: &ID3D11Texture2D,
    ) -> Result<IMFMediaBuffer, EncoderError> {
        unsafe {
            let (width, height) = (self.width as usize, self.height as usize);
            let size = width * height * 3 / 2;
            let buffer = MFCreateMemoryBuffer(size as u32).map_err(encode_error)?;
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();

            self.context.CopyResource(staging, nv12);

            // O `Map` espera a GPU terminar o blit e a cópia: é aqui que o quadro paga a
            // ida à memória, dentro do `busyUs`.
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(encode_error)?;

            let pitch = mapped.RowPitch as usize;
            let mut start = std::ptr::null_mut();
            let locked = buffer.Lock(&mut start, None, None);

            if locked.is_ok() {
                copy_nv12(
                    std::slice::from_raw_parts(mapped.pData.cast::<u8>(), pitch * (height * 3 / 2 - 1) + width),
                    pitch,
                    width,
                    std::slice::from_raw_parts_mut(start, size),
                );
            }

            // Solta a textura antes de qualquer erro subir: mapeada, ela trava a próxima
            // cópia da GPU.
            self.context.Unmap(staging, 0);

            locked.map_err(encode_error)?;
            buffer.Unlock().map_err(encode_error)?;
            buffer.SetCurrentLength(size as u32).map_err(encode_error)?;

            Ok(buffer)
        }
    }

    /// Marca o próximo quadro como chave.
    ///
    /// Silenciosa quando o encoder não implementa: a transmissão continua, só recupera de
    /// uma perda no quadro-chave periódico em vez de na hora. Falhar aqui derrubaria a
    /// transmissão inteira por causa de uma otimização.
    fn mark_keyframe(&self) {
        let Ok(codec) = self.transform.cast::<ICodecAPI>() else {
            return;
        };

        let _ = unsafe {
            codec.SetValue(
                &CODECAPI_AVEncVideoForceKeyFrame,
                &variant(VT_UI4, VARIANT_0_0_0 { ulVal: 1 }),
            )
        };
    }

    unsafe fn build_sample(&self, timestamp_ns: u64) -> Result<IMFSample, EncoderError> {
        unsafe {
            let bridge = self
                .bridge
                .as_ref()
                .ok_or_else(|| EncoderError::Encode("sem ponte para o encoder".into()))?;

            let buffer = match &self.backend {
                Backend::Gpu { .. } => {
                    MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &bridge.nv12, 0, false)
                        .map_err(encode_error)?
                }
                Backend::Cpu { staging, .. } => self.read_back(&bridge.nv12, staging)?,
            };

            let sample = MFCreateSample().map_err(encode_error)?;

            sample.AddBuffer(&buffer).map_err(encode_error)?;

            let duration = (HNS_PER_SECOND as f64 / self.frame_rate).round() as i64;

            sample
                .SetSampleTime((timestamp_ns / 100) as i64)
                .map_err(encode_error)?;
            sample.SetSampleDuration(duration).map_err(encode_error)?;

            Ok(sample)
        }
    }

    /// Roda a fila de eventos do MFT até ele pedir mais entrada.
    ///
    /// Encoder de hardware é assíncrono: não se entrega um quadro e recebe outro na
    /// mesma linha. Ele avisa quando quer entrada e quando tem saída, e é preciso
    /// atender os dois — ignorar um evento trava a fila inteira.
    unsafe fn pump(&mut self, mut input: Option<IMFSample>) -> Result<(), EncoderError> {
        unsafe {
            let Backend::Gpu { events } = &self.backend else {
                return Err(EncoderError::Encode("fila de eventos num MFT síncrono".into()));
            };
            let events = events.clone();
            let mut delivered = false;

            // Pedido guardado de uma chamada anterior: o MFT já disse que quer entrada,
            // então entrega direto em vez de esperar um evento que não virá de novo.
            if self.credits > 0
                && let Some(sample) = input.take()
            {
                self.transform
                    .ProcessInput(0, &sample, 0)
                    .map_err(encode_error)?;

                self.credits -= 1;
                delivered = true;
            }

            // Sai quando houver saída para devolver, ou quando o encoder pedir entrada e
            // não houver mais nenhuma — o que acontece nos primeiros quadros, enquanto
            // ele enche a própria fila. Sem a segunda saída, isto penduraria a captura.
            while !delivered || self.ready.is_empty() {
                let event = events
                    .GetEvent(Default::default())
                    .map_err(encode_error)?;
                let kind = MF_EVENT_TYPE(event.GetType().map_err(encode_error)? as i32);

                if kind == METransformNeedInput {
                    let Some(sample) = input.take() else {
                        // Nada mais a entregar nesta chamada. O pedido fica guardado
                        // para o próximo quadro: descartá-lo era o que travava tudo.
                        self.credits += 1;

                        return Ok(());
                    };

                    self.transform
                        .ProcessInput(0, &sample, 0)
                        .map_err(encode_error)?;

                    delivered = true;
                } else if kind == METransformHaveOutput {
                    self.collect_output()?;
                }
            }

            Ok(())
        }
    }

    /// Pega uma saída do MFT, se houver. Devolve se pegou.
    unsafe fn collect_output(&mut self) -> Result<bool, EncoderError> {
        unsafe {
            let mut output = [MFT_OUTPUT_DATA_BUFFER::default()];
            let mut status = 0_u32;

            let info = self.transform.GetOutputStreamInfo(0).map_err(encode_error)?;

            if info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 == 0 {
                let buffer = MFCreateMemoryBuffer(info.cbSize.max(self.width * self.height * 3 / 2))
                    .map_err(encode_error)?;
                let sample = MFCreateSample().map_err(encode_error)?;

                sample.AddBuffer(&buffer).map_err(encode_error)?;
                output[0].pSample = std::mem::ManuallyDrop::new(Some(sample));
            }

            let result = self.transform.ProcessOutput(0, &mut output, &mut status);
            let sample = output[0].pSample.take();

            match result {
                Ok(()) => {}
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(false),
                Err(error) => return Err(encode_error(error)),
            }

            let sample = sample
                .ok_or_else(|| EncoderError::Encode("o encoder não devolveu amostra".into()))?;

            let buffer = sample
                .ConvertToContiguousBuffer()
                .map_err(encode_error)?;

            let mut start = std::ptr::null_mut();
            let mut size = 0_u32;

            buffer
                .Lock(&mut start, None, Some(&mut size))
                .map_err(encode_error)?;

            let data = std::slice::from_raw_parts(start, size as usize).to_vec();

            buffer.Unlock().map_err(encode_error)?;

            // Os primeiros quadros saem no log em `debug`: é o que diz, sem adivinhar, se
            // o MFT entregou Annex-B e se o keyframe trouxe SPS e PPS junto.
            if self.described < 3 {
                self.described += 1;

                tracing::debug!(
                    bytes = data.len(),
                    nals = %nal_types(&data),
                    "encoder: como saiu o quadro"
                );
            }

            self.ready.push_back(EncodedFrame {
                keyframe: is_keyframe(&data),
                data,
                timestamp_ns: 0,
            });

            Ok(true)
        }
    }
}


/// Quantos segundos entre quadros-chave. Quem perde um pacote fica congelado até o
/// próximo, então isto é o teto da travada de quem assiste — mas o servidor pede um
/// quadro-chave na hora quando vê buraco, então o periódico só cobre quem acabou de
/// entrar. Dois segundos é metade dos keyframes, e keyframe é o quadro mais caro.
const GOP_SECONDS: f64 = 2.0;

/// O valor booleano do COM para verdadeiro. Nenhum dos ajustes aqui é desligado.
const LIGADO: VARIANT_0_0_0 = VARIANT_0_0_0 {
    boolVal: VARIANT_BOOL(-1),
};

/// Ajusta o controle de taxa do encoder.
///
/// `MF_MT_AVG_BITRATE` no tipo de mídia é só uma dica: sem dizer o **modo**, o MFT de
/// placa escolhe o dele, e o que se via era 11 Mb/s medidos com 7 Mb/s pedidos. Sobra
/// que o uplink engole mas a internet no meio do caminho nem sempre — e cada pacote
/// perdido lá fora congela quem assiste até o quadro-chave seguinte.
///
/// Nada aqui é fatal. Encoder de placa recusa a propriedade que não implementa, e
/// transmitir com o padrão do driver é pior do que com estes valores, mas ainda é
/// transmitir. O que foi recusado vai para o log, porque é a primeira coisa a olhar
/// quando a taxa não bate numa máquina específica.
unsafe fn tune(transform: &IMFTransform, config: &EncoderConfig) {
    let Ok(codec) = transform.cast::<ICodecAPI>() else {
        tracing::warn!("encoder: sem ICodecAPI, o controle de taxa fica no padrão do driver");

        return;
    };

    let gop = (config.frame_rate * GOP_SECONDS).round() as u32;

    let settings: [(&::windows::core::GUID, VARIANT, &str); 9] = [
        (
            &CODECAPI_AVEncCommonRateControlMode,
            variant(VT_UI4, VARIANT_0_0_0 { ulVal: eAVEncCommonRateControlMode_PeakConstrainedVBR.0 as u32 }),
            "modo de taxa",
        ),
        (
            &CODECAPI_AVEncCommonMeanBitRate,
            variant(VT_UI4, VARIANT_0_0_0 { ulVal: config.bitrate }),
            "taxa média",
        ),
        (
            &CODECAPI_AVEncCommonMaxBitRate,
            variant(VT_UI4, VARIANT_0_0_0 { ulVal: config.bitrate * 3 / 2 }),
            "taxa de pico",
        ),
        (
            &CODECAPI_AVEncCommonQualityVsSpeed,
            variant(VT_UI4, VARIANT_0_0_0 { ulVal: 100 }),
            "qualidade sobre velocidade",
        ),
        (
            &CODECAPI_AVEncMPVDefaultBPictureCount,
            variant(VT_UI4, VARIANT_0_0_0 { ulVal: 0 }),
            "sem quadros B",
        ),
        (&CODECAPI_AVEncCommonLowLatency, variant(VT_BOOL, LIGADO), "baixa latência"),
        // Outra chave para a mesma coisa: é esta que o MFT de software do Windows lê. Sem
        // ela ele segurava 16 quadros antes do primeiro sair, meio segundo de atraso fixo.
        (&CODECAPI_AVLowLatencyMode, variant(VT_BOOL, LIGADO), "modo de baixa latência"),
        (&CODECAPI_AVEncCommonRealTime, variant(VT_BOOL, LIGADO), "tempo real"),
        (
            &CODECAPI_AVEncMPVGOPSize,
            variant(VT_UI4, VARIANT_0_0_0 { ulVal: gop }),
            "intervalo de quadro-chave",
        ),
    ];

    for (key, value, name) in settings {
        if let Err(error) = unsafe { codec.SetValue(key, &value) } {
            tracing::warn!(error = %error, ajuste = name, "encoder: ajuste recusado");
        }
    }

    tracing::info!(
        bitrate = config.bitrate,
        gop,
        "encoder: VBR com pico, baixa latência"
    );
}

/// `VARIANT` cru, do jeito que o `ICodecAPI` espera.
///
/// A crate oferece `From<u64>`, que monta `VT_UI8` — e o encoder recusa. Montar campo a
/// campo é o que sobra. Nada aqui aloca, então o `VariantClear` do `Drop` é inócuo.
fn variant(vt: VARENUM, value: VARIANT_0_0_0) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(VARIANT_0_0 {
                vt,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: value,
            }),
        },
    }
}

pub(crate) unsafe fn start_media_foundation() -> Result<(), EncoderError> {
    let mut failure = None;

    MF_STARTUP.call_once(|| {
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) } {
            failure = Some(error);
        }
    });

    match failure {
        Some(error) => Err(start_error(error)),
        None => Ok(()),
    }
}

/// Um device só para codificar, criado com suporte a vídeo.
///
/// Não dá para reaproveitar o da captura: ele nasce sem a flag, e sem ela não há
/// VideoProcessor nem gerente de device. Um device a mais na mesma placa custa memória,
/// não desempenho — a cópia entre os dois nunca sai da GPU.
unsafe fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext), EncoderError> {
    unsafe {
        let mut device = None;
        let mut context = None;

        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .map_err(start_error)?;

        match (device, context) {
            (Some(device), Some(context)) => Ok((device, context)),
            _ => Err(EncoderError::Start(
                "o Direct3D não devolveu device nem contexto".into(),
            )),
        }
    }
}

/// O primeiro MFT de H.264 da categoria pedida que aceitar a configuração inteira.
///
/// `MFT_ENUM_FLAG_HARDWARE` traz os da placa, `MFT_ENUM_FLAG_SYNCMFT` o de software do
/// Windows. O primeiro da lista não basta: num notebook com duas placas o sistema pode
/// anunciar o encoder da NVIDIA na frente enquanto o device é o da Intel, e aí o gerente de
/// device ou os tipos são recusados e a transmissão morria com uma placa boa sobrando. Cada
/// um que recusa vai para o log com o nome, é desligado, e o seguinte tenta.
unsafe fn open_encoder(
    flags: MFT_ENUM_FLAG,
    manager: Option<&IMFDXGIDeviceManager>,
    config: &EncoderConfig,
) -> Result<IMFTransform, EncoderError> {
    unsafe {
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };

        let mut found: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut how_many = 0_u32;

        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut found,
            &mut how_many,
        )
        .map_err(start_error)?;

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
            let name = friendly_name(&activate);

            tracing::info!(name = %name, "encoder: tentando o MFT");

            match try_encoder(&activate, manager, config) {
                Ok(transform) => {
                    tracing::info!(name = %name, "encoder: MFT escolhido");

                    return Ok(transform);
                }
                Err(error) => {
                    tracing::warn!(error = %error, name = %name, "encoder: MFT recusou, tentando o próximo");

                    if let Err(error) = activate.ShutdownObject() {
                        tracing::warn!(error = %error, name = %name, "encoder: MFT recusado não desligou");
                    }
                }
            }
        }

        Err(EncoderError::Start(
            "nenhum encoder de H.264 desta máquina aceitou a configuração".into(),
        ))
    }
}

unsafe fn try_encoder(
    activate: &IMFActivate,
    manager: Option<&IMFDXGIDeviceManager>,
    config: &EncoderConfig,
) -> Result<IMFTransform, EncoderError> {
    unsafe {
        let transform: IMFTransform = activate.ActivateObject().map_err(start_error)?;

        if let Some(manager) = manager {
            // Encoder de hardware nasce trancado: sem destrancar, ele recusa ProcessInput.
            transform
                .GetAttributes()
                .map_err(start_error)?
                .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
                .map_err(start_error)?;

            transform
                .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
                .map_err(start_error)?;
        }

        // O MFT de software do Windows só lê quadros B, baixa latência e modo de taxa antes
        // dos tipos; depois deles ignora calado. Medido: saía com quadros B (amostras fora
        // de ordem) e 17 quadros de fila, meio segundo de atraso. O da placa segue lendo
        // depois dos tipos, a ordem que já estava provada.
        if manager.is_none() {
            tune(&transform, config);
        }

        tracing::info!(width = config.width, height = config.height, "encoder: configurando os tipos de mídia");

        configure_types(&transform, config.width, config.height, config)?;

        if manager.is_some() {
            tune(&transform, config);
        }

        tracing::info!("encoder: começando o streaming do MFT");

        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
            .map_err(start_error)?;
        transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
            .map_err(start_error)?;

        Ok(transform)
    }
}

/// O nome que o fabricante deu ao MFT — "NVIDIA H.264 Encoder MFT", "H264 Encoder MFT". É o
/// que diz no log qual placa ficou com a transmissão.
unsafe fn friendly_name(activate: &IMFActivate) -> String {
    unsafe {
        let mut name = PWSTR::null();
        let mut length = 0_u32;

        if activate
            .GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut length)
            .is_err()
        {
            return "sem nome".into();
        }

        let text = name.to_string().unwrap_or_default();

        CoTaskMemFree(Some(name.0.cast::<core::ffi::c_void>().cast_const()));

        text
    }
}

/// Textura NV12 no device do encoder. A de saída do VideoProcessor mora só na GPU; a de
/// `staging` é a cópia que a CPU consegue ler, para o MFT de software.
unsafe fn nv12_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    staging: bool,
) -> Result<ID3D11Texture2D, EncoderError> {
    unsafe {
        let descriptor = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_NV12,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: if staging { D3D11_USAGE_STAGING } else { D3D11_USAGE_DEFAULT },
            BindFlags: if staging { 0 } else { D3D11_BIND_RENDER_TARGET.0 as u32 },
            CPUAccessFlags: if staging { D3D11_CPU_ACCESS_READ.0 as u32 } else { 0 },
            MiscFlags: 0,
        };

        let mut texture: Option<ID3D11Texture2D> = None;

        device
            .CreateTexture2D(&descriptor, None, Some(&mut texture))
            .map_err(encode_error)?;

        texture.ok_or_else(|| EncoderError::Encode("a textura NV12 não foi criada".into()))
    }
}

/// Linhas de NV12 (Y inteiro, depois UV pela metade) de uma textura mapeada para um buffer
/// contíguo. A textura tem `pitch` bytes por linha — o driver alinha, e o que passa de
/// `width` é enchimento que o encoder leria como imagem.
fn copy_nv12(source: &[u8], pitch: usize, width: usize, destination: &mut [u8]) {
    for (target, row) in destination.chunks_exact_mut(width).zip(source.chunks(pitch)) {
        target.copy_from_slice(&row[..width]);
    }
}

/// A saída primeiro, a entrada depois: o MFT recusa a entrada enquanto não souber o que
/// tem de produzir.
unsafe fn configure_types(
    transform: &IMFTransform,
    width: u32,
    height: u32,
    config: &EncoderConfig,
) -> Result<(), EncoderError> {
    unsafe {
        let rate = config.frame_rate.round() as u32;

        let output_type = |with_color: bool| -> Result<IMFMediaType, EncoderError> {
            let output: IMFMediaType = MFCreateMediaType().map_err(start_error)?;

            output
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(start_error)?;
            output
                .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
                .map_err(start_error)?;
            output
                .SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate)
                .map_err(start_error)?;
            output
                .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .map_err(start_error)?;
            output
                .SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 0)
                .map_err(start_error)?;
            set_frame_size(&output, &MF_MT_FRAME_SIZE, width, height)?;
            set_ratio(&output, &MF_MT_FRAME_RATE, rate, 1)?;

            if with_color {
                set_color(&output)?;
            }

            Ok(output)
        };

        match transform.SetOutputType(0, Some(&output_type(true)?), 0) {
            Ok(()) => tracing::info!("encoder: tipo de saída com atributos de cor"),
            Err(error) => {
                tracing::warn!(error = %error, "encoder: tipo de saída recusado com cor, tentando sem");

                transform
                    .SetOutputType(0, Some(&output_type(false)?), 0)
                    .map_err(start_error)?;
            }
        }

        let input: IMFMediaType = MFCreateMediaType().map_err(start_error)?;

        input
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(start_error)?;
        input
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(start_error)?;
        input
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(start_error)?;
        set_frame_size(&input, &MF_MT_FRAME_SIZE, width, height)?;
        set_ratio(&input, &MF_MT_FRAME_RATE, rate, 1)?;
        set_color(&input)?;

        transform
            .SetInputType(0, Some(&input), 0)
            .map_err(start_error)?;

        Ok(())
    }
}

/// O mesmo espaço de cor que o processador produz, dito ao encoder para que ele escreva
/// o VUI no SPS. É o VUI que faz o decodificador do outro lado usar a matriz e a faixa
/// certas em vez de chutar.
unsafe fn set_color(kind: &IMFMediaType) -> Result<(), EncoderError> {
    unsafe {
        kind.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
            .map_err(start_error)?;
        kind.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
            .map_err(start_error)?;
        kind.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)
            .map_err(start_error)?;
        kind.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)
            .map_err(start_error)?;

        Ok(())
    }
}

/// `D3D11_VIDEO_PROCESSOR_COLOR_SPACE` é um campo de bits que a crate expõe cru:
/// `Usage` no bit 0 (0 = reprodução), `RGB_Range` no bit 1 (0 = 0–255),
/// `YCbCr_Matrix` no bit 2 (1 = BT.709), `YCbCr_xvYCC` no bit 3 e `Nominal_Range` nos
/// bits 4–5. Só a faixa nominal varia entre entrada e saída.
fn color_space(range: D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE) -> D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
    const MATRIX_BT709: u32 = 1 << 2;

    D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
        _bitfield: MATRIX_BT709 | ((range.0 as u32) << 4),
    }
}

/// Largura e altura moram num atributo só, empacotadas em 64 bits.
unsafe fn set_frame_size(
    kind: &IMFMediaType,
    key: &::windows::core::GUID,
    width: u32,
    height: u32,
) -> Result<(), EncoderError> {
    unsafe {
        kind.SetUINT64(key, (u64::from(width) << 32) | u64::from(height))
            .map_err(start_error)
    }
}

unsafe fn set_ratio(
    kind: &IMFMediaType,
    key: &::windows::core::GUID,
    numerator: u32,
    denominator: u32,
) -> Result<(), EncoderError> {
    unsafe {
        kind.SetUINT64(key, (u64::from(numerator) << 32) | u64::from(denominator))
            .map_err(start_error)
    }
}

/// Um keyframe de H.264 carrega SPS (tipo 7), PPS (8) ou IDR (5). O MFT entrega em
/// Annex-B, com prefixo `00 00 00 01`.
fn is_keyframe(data: &[u8]) -> bool {
    let mut position = 0;

    while position + 4 < data.len() {
        if data[position] == 0
            && data[position + 1] == 0
            && data[position + 2] == 0
            && data[position + 3] == 1
        {
            if matches!(
                data.get(position + 4).map(|byte| byte & 0x1F),
                Some(5 | 7 | 8)
            ) {
                return true;
            }

            position += 4;

            continue;
        }

        position += 1;
    }

    false
}

/// Os tipos de NAL do quadro, na ordem, e o tamanho de cada um. Sem prefixo Annex-B a
/// lista sai vazia — e é isso que se quer saber.
fn nal_types(data: &[u8]) -> String {
    let mut found = Vec::new();
    let mut position = 0;

    while position + 4 < data.len() {
        if data[position] == 0 && data[position + 1] == 0 {
            let prefix = if data[position + 2] == 1 {
                3
            } else if data[position + 2] == 0 && data[position + 3] == 1 {
                4
            } else {
                position += 1;

                continue;
            };

            if let Some(byte) = data.get(position + prefix) {
                found.push((byte & 0x1F).to_string());
            }

            position += prefix;

            continue;
        }

        position += 1;
    }

    if found.is_empty() {
        return format!("sem Annex-B, começa com {:02x?}", &data[..data.len().min(8)]);
    }

    found.join(",")
}

fn start_error(error: ::windows::core::Error) -> EncoderError {
    EncoderError::Start(error.message())
}

fn encode_error(error: ::windows::core::Error) -> EncoderError {
    EncoderError::Encode(error.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_space_packs_the_bitfield_like_d3d11_h() {
        // Entrada: RGB cheio, BT.709, 0–255 → Usage 0, RGB_Range 0, matriz 1, faixa 2.
        assert_eq!(color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_0_255)._bitfield, 0b10_0100);
        // Saída: BT.709, 16–235 → faixa 1.
        assert_eq!(color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235)._bitfield, 0b01_0100);
    }

    #[test]
    fn nv12_rows_leave_the_texture_padding_behind() {
        let source = [1, 2, 3, 4, 0xEE, 0xEE, 5, 6, 7, 8, 0xEE, 0xEE, 9, 10, 11, 12];
        let mut destination = [0; 12];

        copy_nv12(&source, 6, 4, &mut destination);

        assert_eq!(destination, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }
}
