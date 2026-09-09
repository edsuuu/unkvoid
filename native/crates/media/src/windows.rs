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
    ponte: Option<Ponte>,
    /// Um encoder de hardware tem fila: nem todo quadro que entra sai no mesmo instante.
    prontos: VecDeque<EncodedFrame>,
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
struct Ponte {
    origem: (u32, u32),
    compartilhada_na_captura: ID3D11Texture2D,
    trava_da_captura: IDXGIKeyedMutex,
    minha_trava: IDXGIKeyedMutex,
    processador: ID3D11VideoProcessor,
    entrada: ID3D11VideoProcessorInputView,
    saida: ID3D11VideoProcessorOutputView,
    nv12: ID3D11Texture2D,
}

impl MediaFoundationEncoder {
    pub fn new(config: &EncoderConfig) -> Result<Self, EncoderError> {
        let (width, height) = config.quality.dimensions();

        unsafe {
            iniciar_media_foundation()?;

            let (device, context) = criar_device()?;

            // Sem isto, o MFT tocando no device de outra thread corrompe o estado dele.
            let multithread: ID3D11Multithread = device.cast().map_err(erro_de_inicio)?;
            let _ = multithread.SetMultithreadProtected(true);

            let video_device: ID3D11VideoDevice = device.cast().map_err(erro_de_inicio)?;
            let video_context: ID3D11VideoContext = context.cast().map_err(erro_de_inicio)?;

            let transform = encoder_de_hardware()?;

            // O gerente é como o MFT descobre em qual placa a textura vive. Sem ele, o
            // encoder recusa qualquer amostra que não esteja na memória do processador.
            let mut token = 0_u32;
            let mut gerente: Option<IMFDXGIDeviceManager> = None;

            MFCreateDXGIDeviceManager(&mut token, &mut gerente).map_err(erro_de_inicio)?;

            let gerente = gerente.ok_or_else(|| {
                EncoderError::Start("o Media Foundation não devolveu o gerente de device".into())
            })?;

            gerente
                .ResetDevice(&device, token)
                .map_err(erro_de_inicio)?;

            transform
                .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, gerente.as_raw() as usize)
                .map_err(erro_de_inicio)?;

            configurar_tipos(&transform, width, height, config)?;

            let events: IMFMediaEventGenerator = transform.cast().map_err(erro_de_inicio)?;

            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(erro_de_inicio)?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(erro_de_inicio)?;

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
                ponte: None,
                prontos: VecDeque::new(),
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
            self.atravessar_a_ponte(surface)?;

            let amostra = self.montar_amostra()?;

            self.frames += 1;

            self.bombear(Some(amostra))?;
        }

        self.prontos
            .pop_front()
            .map(|quadro| EncodedFrame {
                timestamp_ns,
                ..quadro
            })
            .ok_or(EncoderError::NeedsMoreInput)
    }

    /// Leva o quadro do device da captura para este, já convertido e no tamanho pedido.
    ///
    /// A conversão e a escala são um blit do VideoProcessor: a captura entrega BGRA no
    /// tamanho nativo do monitor, o encoder quer NV12 no tamanho escolhido, e fazer essa
    /// conta no processador devolveria o problema de fps que o app existe para resolver.
    unsafe fn atravessar_a_ponte(&mut self, surface: &GpuSurface) -> Result<(), EncoderError> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();

        unsafe { surface.texture.GetDesc(&mut desc) };

        let origem = (desc.Width, desc.Height);

        if self
            .ponte
            .as_ref()
            .is_none_or(|ponte| ponte.origem != origem)
        {
            self.ponte = Some(unsafe { self.montar_ponte(surface, origem)? });
        }

        let ponte = self.ponte.as_ref().expect("acabou de ser montada");

        unsafe {
            // Chave 0 é o lado da captura, chave 1 é o meu: a trava alterna entre os dois
            // e é o que garante que a cópia terminou antes do blit começar.
            ponte
                .trava_da_captura
                .AcquireSync(0, u32::MAX)
                .map_err(erro_de_encode)?;

            surface
                .context
                .CopyResource(&ponte.compartilhada_na_captura, &surface.texture);

            // A cópia é assíncrona na GPU. Liberar a mutex antes do Flush deixava o
            // encoder ler a textura compartilhada antes de a captura terminar de
            // preenchê-la, produzindo vídeo preto apesar de o preview estar correto.
            surface.context.Flush();

            ponte
                .trava_da_captura
                .ReleaseSync(1)
                .map_err(erro_de_encode)?;

            ponte
                .minha_trava
                .AcquireSync(1, u32::MAX)
                .map_err(erro_de_encode)?;

            // `ManuallyDrop` porque o campo é dono do ponteiro: sem isto a struct
            // liberaria a view ao sair de escopo, e ela pertence à ponte.
            let fluxo = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: std::mem::ManuallyDrop::new(Some(ponte.entrada.clone())),
                ..Default::default()
            };

            self.video_context
                .VideoProcessorBlt(&ponte.processador, &ponte.saida, 0, &[fluxo])
                .map_err(erro_de_encode)?;

            ponte.minha_trava.ReleaseSync(0).map_err(erro_de_encode)?;
        }

        Ok(())
    }

    unsafe fn montar_ponte(
        &self,
        surface: &GpuSurface,
        origem: (u32, u32),
    ) -> Result<Ponte, EncoderError> {
        unsafe {
            let descricao = D3D11_TEXTURE2D_DESC {
                Width: origem.0,
                Height: origem.1,
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

            let mut compartilhada: Option<ID3D11Texture2D> = None;

            surface
                .device
                .CreateTexture2D(&descricao, None, Some(&mut compartilhada))
                .map_err(erro_de_encode)?;

            let compartilhada = compartilhada.ok_or_else(|| {
                EncoderError::Encode("a textura compartilhada não foi criada".into())
            })?;

            let recurso: IDXGIResource = compartilhada.cast().map_err(erro_de_encode)?;
            let identificador: HANDLE = recurso.GetSharedHandle().map_err(erro_de_encode)?;

            let mut minha: Option<ID3D11Texture2D> = None;

            self.device
                .OpenSharedResource(identificador, &mut minha)
                .map_err(erro_de_encode)?;

            let minha = minha.ok_or_else(|| {
                EncoderError::Encode("a textura compartilhada não abriu neste device".into())
            })?;

            let trava_da_captura: IDXGIKeyedMutex = compartilhada.cast().map_err(erro_de_encode)?;
            let minha_trava: IDXGIKeyedMutex = minha.cast().map_err(erro_de_encode)?;

            let (processador, enumerador) = self.criar_processador(origem)?;
            let nv12 = self.criar_nv12()?;

            let entrada_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };

            let mut entrada: Option<ID3D11VideoProcessorInputView> = None;

            self.video_device
                .CreateVideoProcessorInputView(
                    &minha,
                    &enumerador,
                    &entrada_desc,
                    Some(&mut entrada),
                )
                .map_err(erro_de_encode)?;

            let saida_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                ..Default::default()
            };

            let mut saida: Option<ID3D11VideoProcessorOutputView> = None;

            self.video_device
                .CreateVideoProcessorOutputView(&nv12, &enumerador, &saida_desc, Some(&mut saida))
                .map_err(erro_de_encode)?;

            Ok(Ponte {
                origem,
                compartilhada_na_captura: compartilhada,
                trava_da_captura,
                minha_trava,
                processador,
                entrada: entrada.ok_or_else(|| {
                    EncoderError::Encode("a view de entrada não foi criada".into())
                })?,
                saida: saida
                    .ok_or_else(|| EncoderError::Encode("a view de saída não foi criada".into()))?,
                nv12,
            })
        }
    }

    unsafe fn criar_processador(
        &self,
        origem: (u32, u32),
    ) -> Result<(ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator), EncoderError> {
        unsafe {
            let taxa = DXGI_RATIONAL {
                Numerator: self.frame_rate.round() as u32,
                Denominator: 1,
            };

            let conteudo = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: taxa,
                InputWidth: origem.0,
                InputHeight: origem.1,
                OutputFrameRate: taxa,
                OutputWidth: self.width,
                OutputHeight: self.height,
                Usage: Default::default(),
            };

            let enumerador = self
                .video_device
                .CreateVideoProcessorEnumerator(&conteudo)
                .map_err(erro_de_encode)?;

            let processador = self
                .video_device
                .CreateVideoProcessor(&enumerador, 0)
                .map_err(erro_de_encode)?;

            Ok((processador, enumerador))
        }
    }

    unsafe fn criar_nv12(&self) -> Result<ID3D11Texture2D, EncoderError> {
        unsafe {
            let descricao = D3D11_TEXTURE2D_DESC {
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

            let mut textura: Option<ID3D11Texture2D> = None;

            self.device
                .CreateTexture2D(&descricao, None, Some(&mut textura))
                .map_err(erro_de_encode)?;

            textura.ok_or_else(|| EncoderError::Encode("a textura NV12 não foi criada".into()))
        }
    }

    unsafe fn montar_amostra(&self) -> Result<IMFSample, EncoderError> {
        unsafe {
            let ponte = self
                .ponte
                .as_ref()
                .ok_or_else(|| EncoderError::Encode("sem ponte para o encoder".into()))?;

            let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &ponte.nv12, 0, false)
                .map_err(erro_de_encode)?;

            let amostra = MFCreateSample().map_err(erro_de_encode)?;

            amostra.AddBuffer(&buffer).map_err(erro_de_encode)?;

            // O tempo é contado em quadros, não no relógio: o encoder precisa de um
            // passo constante, e o relógio da captura varia com a carga da máquina.
            let duracao = (HNS_PER_SECOND as f64 / self.frame_rate).round() as i64;

            amostra
                .SetSampleTime(self.frames * duracao)
                .map_err(erro_de_encode)?;
            amostra.SetSampleDuration(duracao).map_err(erro_de_encode)?;

            Ok(amostra)
        }
    }

    /// Roda a fila de eventos do MFT até ele pedir mais entrada.
    ///
    /// Encoder de hardware é assíncrono: não se entrega um quadro e recebe outro na
    /// mesma linha. Ele avisa quando quer entrada e quando tem saída, e é preciso
    /// atender os dois — ignorar um evento trava a fila inteira.
    unsafe fn bombear(&mut self, mut entrada: Option<IMFSample>) -> Result<(), EncoderError> {
        unsafe {
            let mut entregue = false;

            // Sai quando houver saída para devolver, ou quando o encoder pedir entrada e
            // não houver mais nenhuma — o que acontece nos primeiros quadros, enquanto
            // ele enche a própria fila. Sem a segunda saída, isto penduraria a captura.
            while !entregue || self.prontos.is_empty() {
                let evento = self
                    .events
                    .GetEvent(Default::default())
                    .map_err(erro_de_encode)?;
                let tipo = MF_EVENT_TYPE(evento.GetType().map_err(erro_de_encode)? as i32);

                if tipo == METransformNeedInput {
                    let Some(amostra) = entrada.take() else {
                        return Ok(());
                    };

                    self.transform
                        .ProcessInput(0, &amostra, 0)
                        .map_err(erro_de_encode)?;

                    entregue = true;
                } else if tipo == METransformHaveOutput {
                    self.recolher()?;
                }
            }

            Ok(())
        }
    }

    unsafe fn recolher(&mut self) -> Result<(), EncoderError> {
        unsafe {
            let mut saida = [MFT_OUTPUT_DATA_BUFFER::default()];
            let mut status = 0_u32;

            match self.transform.ProcessOutput(0, &mut saida, &mut status) {
                Ok(()) => {}
                Err(erro) if erro.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                Err(erro) => return Err(erro_de_encode(erro)),
            }

            let amostra = saida[0]
                .pSample
                .take()
                .ok_or_else(|| EncoderError::Encode("o encoder não devolveu amostra".into()))?;

            let buffer = amostra
                .ConvertToContiguousBuffer()
                .map_err(erro_de_encode)?;

            let mut inicio = std::ptr::null_mut();
            let mut tamanho = 0_u32;

            buffer
                .Lock(&mut inicio, None, Some(&mut tamanho))
                .map_err(erro_de_encode)?;

            let dados = std::slice::from_raw_parts(inicio, tamanho as usize).to_vec();

            buffer.Unlock().map_err(erro_de_encode)?;

            self.prontos.push_back(EncodedFrame {
                keyframe: e_keyframe(&dados),
                data: dados,
                timestamp_ns: 0,
            });

            Ok(())
        }
    }
}

unsafe fn iniciar_media_foundation() -> Result<(), EncoderError> {
    let mut falha = None;

    MF_STARTUP.call_once(|| {
        if let Err(erro) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) } {
            falha = Some(erro);
        }
    });

    match falha {
        Some(erro) => Err(erro_de_inicio(erro)),
        None => Ok(()),
    }
}

/// Um device só para codificar, criado com suporte a vídeo.
///
/// Não dá para reaproveitar o da captura: ele nasce sem a flag, e sem ela não há
/// VideoProcessor nem gerente de device. Um device a mais na mesma placa custa memória,
/// não desempenho — a cópia entre os dois nunca sai da GPU.
unsafe fn criar_device() -> Result<(ID3D11Device, ID3D11DeviceContext), EncoderError> {
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
        .map_err(erro_de_inicio)?;

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
unsafe fn encoder_de_hardware() -> Result<IMFTransform, EncoderError> {
    unsafe {
        let entrada = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let saida = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };

        let mut encontrados: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut quantos = 0_u32;

        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&entrada),
            Some(&saida),
            &mut encontrados,
            &mut quantos,
        )
        .map_err(erro_de_inicio)?;

        if quantos == 0 {
            return Err(EncoderError::Start(
                "esta máquina não tem encoder de H.264 por hardware".into(),
            ));
        }

        let lista = std::slice::from_raw_parts(encontrados, quantos as usize);
        let primeiro = lista[0]
            .clone()
            .ok_or_else(|| EncoderError::Start("a lista de encoders veio vazia".into()))?;

        let transform: IMFTransform = primeiro.ActivateObject().map_err(erro_de_inicio)?;

        // Encoder de hardware nasce trancado: sem destrancar, ele recusa ProcessInput.
        let atributos = transform.GetAttributes().map_err(erro_de_inicio)?;

        atributos
            .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
            .map_err(erro_de_inicio)?;

        Ok(transform)
    }
}

/// A saída primeiro, a entrada depois: o MFT recusa a entrada enquanto não souber o que
/// tem de produzir.
unsafe fn configurar_tipos(
    transform: &IMFTransform,
    width: u32,
    height: u32,
    config: &EncoderConfig,
) -> Result<(), EncoderError> {
    unsafe {
        let taxa = config.frame_rate.round() as u32;

        let saida: IMFMediaType = MFCreateMediaType().map_err(erro_de_inicio)?;

        saida
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(erro_de_inicio)?;
        saida
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
            .map_err(erro_de_inicio)?;
        saida
            .SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate)
            .map_err(erro_de_inicio)?;
        saida
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(erro_de_inicio)?;
        saida
            .SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 0)
            .map_err(erro_de_inicio)?;
        definir_tamanho(&saida, &MF_MT_FRAME_SIZE, width, height)?;
        definir_razao(&saida, &MF_MT_FRAME_RATE, taxa, 1)?;

        transform
            .SetOutputType(0, Some(&saida), 0)
            .map_err(erro_de_inicio)?;

        let entrada: IMFMediaType = MFCreateMediaType().map_err(erro_de_inicio)?;

        entrada
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(erro_de_inicio)?;
        entrada
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
            .map_err(erro_de_inicio)?;
        entrada
            .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
            .map_err(erro_de_inicio)?;
        definir_tamanho(&entrada, &MF_MT_FRAME_SIZE, width, height)?;
        definir_razao(&entrada, &MF_MT_FRAME_RATE, taxa, 1)?;

        transform
            .SetInputType(0, Some(&entrada), 0)
            .map_err(erro_de_inicio)?;

        Ok(())
    }
}

/// Largura e altura moram num atributo só, empacotadas em 64 bits.
unsafe fn definir_tamanho(
    tipo: &IMFMediaType,
    chave: &::windows::core::GUID,
    largura: u32,
    altura: u32,
) -> Result<(), EncoderError> {
    unsafe {
        tipo.SetUINT64(chave, (u64::from(largura) << 32) | u64::from(altura))
            .map_err(erro_de_inicio)
    }
}

unsafe fn definir_razao(
    tipo: &IMFMediaType,
    chave: &::windows::core::GUID,
    numerador: u32,
    denominador: u32,
) -> Result<(), EncoderError> {
    unsafe {
        tipo.SetUINT64(chave, (u64::from(numerador) << 32) | u64::from(denominador))
            .map_err(erro_de_inicio)
    }
}

/// Um keyframe de H.264 carrega SPS (tipo 7), PPS (8) ou IDR (5). O MFT entrega em
/// Annex-B, com prefixo `00 00 00 01`.
fn e_keyframe(dados: &[u8]) -> bool {
    let mut posicao = 0;

    while posicao + 4 < dados.len() {
        if dados[posicao] == 0
            && dados[posicao + 1] == 0
            && dados[posicao + 2] == 0
            && dados[posicao + 3] == 1
        {
            if matches!(
                dados.get(posicao + 4).map(|byte| byte & 0x1F),
                Some(5 | 7 | 8)
            ) {
                return true;
            }

            posicao += 4;

            continue;
        }

        posicao += 1;
    }

    false
}

fn erro_de_inicio(erro: ::windows::core::Error) -> EncoderError {
    EncoderError::Start(erro.message())
}

fn erro_de_encode(erro: ::windows::core::Error) -> EncoderError {
    EncoderError::Encode(erro.message())
}
