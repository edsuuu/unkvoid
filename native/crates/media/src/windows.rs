//! Encoder de hardware no Windows: Media Foundation por cima da placa de vídeo.
//!
//! O quadro chega como textura do Direct3D, já na GPU, e sai como H.264 sem nunca
//! passar pela CPU — que é o ponto do app inteiro: comprimir 1080p60 no processador
//! rouba do jogo exatamente o que ele precisa.
//!
//! O MFT de H.264 por hardware é o mesmo caminho que o NVENC (NVIDIA), o QuickSync
//! (Intel) e o VCE (AMD) expõem ao Windows. Quem escolhe é o sistema.

use std::collections::VecDeque;
use std::sync::Once;

use ::windows::Win32::Foundation::HANDLE;
use ::windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_1};
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
    ID3D11VideoProcessorInputView, ID3D11VideoProcessorOutputView,
};
use ::windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use ::windows::Win32::Graphics::Dxgi::{IDXGIKeyedMutex, IDXGIResource};
use ::windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFDXGIDeviceManager, IMFMediaEventGenerator, IMFMediaType, IMFSample,
    IMFTransform, METransformHaveOutput, METransformNeedInput, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_EVENT_TYPE, MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE,
    MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION, MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer,
    MFCreateMediaType, MFCreateSample, MFMediaType_Video, MFSTARTUP_NOSOCKET, MFStartup,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER, MFT_REGISTER_TYPE_INFO, MFTEnumEx,
    MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
};
use ::windows::Win32::System::Com::CoTaskMemFree;
use ::windows::core::Interface;

use crate::{EncodedFrame, EncoderConfig, EncoderError, GpuSurface};

/// A unidade de tempo do Media Foundation: 100 nanossegundos.
const HNS_PER_SECOND: i64 = 10_000_000;

/// `MFStartup` é por processo, e chamar duas vezes devolve erro.
static MF_STARTUP: Once = Once::new();

pub struct MediaFoundationEncoder {
    device: ID3D11Device,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    width: u32,
    height: u32,
    frame_rate: f64,
    frames: i64,
    bridge: Option<Bridge>,
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
        let (width, height) = config.quality.dimensions();

        // Um passo por linha, anunciado antes de acontecer. Daqui para baixo é tudo COM
        // e Direct3D: driver velho, placa sem encoder de hardware ou sessão sem GPU não
        // devolvem erro — derrubam o processo. A última linha no arquivo é o passo que
        // matou.
        unsafe {
            tracing::info!("encoder: MFStartup");

            start_media_foundation()?;

            tracing::info!("encoder: criando o device do Direct3D 11");

            let (device, context) = create_device()?;

            // Sem isto, o MFT tocando no device de outra thread corrompe o estado dele.
            let multithread: ID3D11Multithread = device.cast().map_err(start_error)?;
            let _ = multithread.SetMultithreadProtected(true);

            let video_device: ID3D11VideoDevice = device.cast().map_err(start_error)?;
            let video_context: ID3D11VideoContext = context.cast().map_err(start_error)?;

            tracing::info!("encoder: procurando o MFT de H.264 por hardware");

            let transform = hardware_encoder()?;

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

            transform
                .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
                .map_err(start_error)?;

            tracing::info!(width, height, "encoder: configurando os tipos de mídia");

            configure_types(&transform, width, height, config)?;

            let events: IMFMediaEventGenerator = transform.cast().map_err(start_error)?;

            tracing::info!("encoder: começando o streaming do MFT");

            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(start_error)?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(start_error)?;

            tracing::info!("encoder: pronto");

            Ok(Self {
                device,
                video_device,
                video_context,
                transform,
                events,
                width,
                height,
                frame_rate: config.frame_rate,
                frames: 0,
                bridge: None,
                ready: VecDeque::new(),
                credits: 0,
            })
        }
    }

    /// Codifica um quadro. `surface` vem da captura sem passar pela CPU.
    pub fn encode(
        &mut self,
        surface: &GpuSurface,
        timestamp_ns: u64,
    ) -> Result<EncodedFrame, EncoderError> {
        unsafe {
            self.cross_the_bridge(surface)?;

            let sample = self.build_sample()?;

            self.frames += 1;

            self.pump(Some(sample))?;
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
                .AcquireSync(0, u32::MAX)
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
                .AcquireSync(1, u32::MAX)
                .map_err(encode_error)?;

            // `ManuallyDrop` porque o campo é dono do ponteiro: sem isto a struct
            // liberaria a view ao sair de escopo, e ela pertence à ponte.
            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(bridge.input.clone())),
                ..Default::default()
            };

            self.video_context
                .VideoProcessorBlt(&bridge.processor, &bridge.output, 0, &[stream])
                .map_err(encode_error)?;

            bridge.my_lock.ReleaseSync(0).map_err(encode_error)?;
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
                Usage: Default::default(),
            };

            let enumerator = self
                .video_device
                .CreateVideoProcessorEnumerator(&content)
                .map_err(encode_error)?;

            let processor = self
                .video_device
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(encode_error)?;

            Ok((processor, enumerator))
        }
    }

    unsafe fn create_nv12(&self) -> Result<ID3D11Texture2D, EncoderError> {
        unsafe {
            let descriptor = D3D11_TEXTURE2D_DESC {
                Width: self.width,
                Height: self.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };

            let mut texture: Option<ID3D11Texture2D> = None;

            self.device
                .CreateTexture2D(&descriptor, None, Some(&mut texture))
                .map_err(encode_error)?;

            texture.ok_or_else(|| EncoderError::Encode("a textura NV12 não foi criada".into()))
        }
    }

    unsafe fn build_sample(&self) -> Result<IMFSample, EncoderError> {
        unsafe {
            let bridge = self
                .bridge
                .as_ref()
                .ok_or_else(|| EncoderError::Encode("sem ponte para o encoder".into()))?;

            let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &bridge.nv12, 0, false)
                .map_err(encode_error)?;

            let sample = MFCreateSample().map_err(encode_error)?;

            sample.AddBuffer(&buffer).map_err(encode_error)?;

            // O tempo é contado em quadros, não no relógio: o encoder precisa de um
            // passo constante, e o relógio da captura varia com a carga da máquina.
            let duration = (HNS_PER_SECOND as f64 / self.frame_rate).round() as i64;

            sample
                .SetSampleTime(self.frames * duration)
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
                let event = self
                    .events
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

    unsafe fn collect_output(&mut self) -> Result<(), EncoderError> {
        unsafe {
            let mut output = [MFT_OUTPUT_DATA_BUFFER::default()];
            let mut status = 0_u32;

            match self.transform.ProcessOutput(0, &mut output, &mut status) {
                Ok(()) => {}
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                Err(error) => return Err(encode_error(error)),
            }

            let sample = output[0]
                .pSample
                .take()
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

            self.ready.push_back(EncodedFrame {
                keyframe: is_keyframe(&data),
                data: data,
                timestamp_ns: 0,
            });

            Ok(())
        }
    }
}

unsafe fn start_media_foundation() -> Result<(), EncoderError> {
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

/// O primeiro MFT de H.264 por hardware que o sistema anunciar.
///
/// `MFT_ENUM_FLAG_HARDWARE` é o que separa o encoder da placa do de software: sem ele o
/// Windows entrega o encoder por CPU, que funciona e é exatamente o que não queremos.
unsafe fn hardware_encoder() -> Result<IMFTransform, EncoderError> {
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
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut found,
            &mut how_many,
        )
        .map_err(start_error)?;

        if found.is_null() {
            return Err(EncoderError::Start(
                "esta máquina não tem encoder de H.264 por hardware".into(),
            ));
        }

        // O `MFTEnumEx` devolve um vetor do alocador COM com uma referência para cada
        // encoder. Quem chamou é dono das duas coisas: das referências e do vetor. Antes
        // daqui saía um `clone` — que soma mais uma referência — e nada era liberado,
        // então cada abertura de transmissão deixava para trás o vetor inteiro e um
        // objeto COM por encoder instalado na máquina.
        let list = std::slice::from_raw_parts_mut(found, how_many as usize);
        let first = list.first_mut().and_then(Option::take);

        // Os outros são liberados aqui; o vetor, logo depois. Vale inclusive quando não
        // veio nenhum: o alocador entrega o vetor do mesmo jeito.
        for slot in list.iter_mut().skip(1) {
            drop(slot.take());
        }

        CoTaskMemFree(Some(found.cast::<core::ffi::c_void>().cast_const()));

        let first = first.ok_or_else(|| {
            EncoderError::Start("esta máquina não tem encoder de H.264 por hardware".into())
        })?;

        let transform: IMFTransform = first.ActivateObject().map_err(start_error)?;

        // Encoder de hardware nasce trancado: sem destrancar, ele recusa ProcessInput.
        let attributes = transform.GetAttributes().map_err(start_error)?;

        attributes
            .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
            .map_err(start_error)?;

        Ok(transform)
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

        transform
            .SetOutputType(0, Some(&output), 0)
            .map_err(start_error)?;

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

        transform
            .SetInputType(0, Some(&input), 0)
            .map_err(start_error)?;

        Ok(())
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

fn start_error(error: ::windows::core::Error) -> EncoderError {
    EncoderError::Start(error.message())
}

fn encode_error(error: ::windows::core::Error) -> EncoderError {
    EncoderError::Encode(error.message())
}
