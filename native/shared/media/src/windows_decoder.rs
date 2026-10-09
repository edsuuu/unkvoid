//! Decodificador de H.264 no Windows: o Media Foundation do próprio sistema, para o app
//! assistir sem GStreamer.
//!
//! É o par do `unpack.rs`: ele remonta o quadro Annex-B que chegou pela rede, e isto o
//! transforma em pixels que a interface desenha. O MFT é o da Microsoft, que vem em todo
//! Windows; nenhum driver de placa precisa estar certo para alguém conseguir assistir.
//!
//! Decodifica na placa quando dá (DXVA, ver `Gpu`) e na CPU quando não: device que não abre,
//! MFT que não aceita o gerente do Direct3D, ou uma falha da placa no meio — essa vale para o
//! resto do processo, que passa a abrir todo decodificador na CPU.

use std::mem::ManuallyDrop;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use ::windows::Win32::Foundation::RECT;
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_TEX2D_VPIV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0, D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_0_255,
    D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL, D3D11_VPIV_DIMENSION_TEXTURE2D,
    D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
    ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoContext1, ID3D11VideoDevice,
    ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView,
};
use ::windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P601,
    DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use ::windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFDXGIDeviceManager, MF_SA_D3D11_AWARE, MFCreateDXGIDeviceManager,
    MFT_MESSAGE_SET_D3D_MANAGER,
};
use ::windows::core::Interface;

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
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};

use crate::DecodedFrame;
use crate::windows::{color_space, create_device, start_media_foundation};

/// O relógio do RTP para vídeo, e a unidade de tempo do Media Foundation (100 ns).
const RTP_CLOCK: i64 = 90_000;
const HNS_PER_SECOND: i64 = 10_000_000;

/// Em quantas threads a conversão para RGBA se divide. Numa thread só, um quadro 1080p levava
/// ~10 ms, mais da metade do custo de assistir; quatro cabem em qualquer PC de hoje e deixam
/// o resto dos núcleos para o jogo de quem assiste.
const CONVERSION_THREADS: usize = 4;

/// As threads da conversão, abertas uma vez para o app inteiro. Antes eram quatro threads novas
/// por quadro — 240 por segundo numa tela a 60 fps —, e uma que o sistema recusasse derrubava a
/// thread da tela com ela.
fn conversion_pool() -> Option<&'static ThreadPool> {
    static POOL: OnceLock<Option<ThreadPool>> = OnceLock::new();

    POOL.get_or_init(|| {
        ThreadPoolBuilder::new()
            .num_threads(CONVERSION_THREADS)
            .thread_name(|index| format!("unkvoid-cor-{index}"))
            .build()
            .ok()
    })
    .as_ref()
}

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

/// A placa já falhou decodificando neste processo: os decodificadores seguintes abrem na CPU.
static GPU_FAILED: AtomicBool = AtomicBool::new(false);

/// `UNKVOID_DECODER=cpu` decodifica na CPU, para comparar os dois caminhos na mesma máquina.
fn gpu_allowed() -> bool {
    !GPU_FAILED.load(Ordering::Relaxed) && !std::env::var("UNKVOID_DECODER").is_ok_and(|value| value == "cpu")
}

/// A decodificação na placa (DXVA): o MFT recebe o gerente do Direct3D e devolve cada quadro numa
/// textura NV12; o VideoProcessor converte para RGBA na própria placa, e só a imagem pronta desce,
/// uma vez por quadro mostrado. Na CPU, decodificar e converter uma tela 1080p60 custava perto de
/// um núcleo, e uma 4K quatro — num PC de quatro núcleos com o jogo aberto, era a diferença entre
/// acompanhar e congelar. Até um Intel integrado antigo decodifica H.264 4K no chip de vídeo.
struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    manager: IMFDXGIDeviceManager,
    converter: Option<Converter>,
}

/// A conversão de um tamanho: refeita quando a textura ou a imagem mudam de tamanho.
struct Converter {
    /// O tamanho da textura do decodificador (com o enchimento, 1088 para 1080), o da imagem e a
    /// matriz de cor que o vídeo declarou.
    sizes: ((u32, u32), (u32, u32), Matrix),
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    output: ID3D11Texture2D,
    output_view: ID3D11VideoProcessorOutputView,
    staging: ID3D11Texture2D,
    /// A placa só converte para BGRA: a troca de canal vai na cópia para a imagem.
    swapped: bool,
}

impl Gpu {
    fn open() -> Result<Self> {
        unsafe {
            let (device, context) = create_device().map_err(|failure| anyhow!("{failure}"))?;

            let _ = device.cast::<ID3D11Multithread>().context("sem proteção multithread")?.SetMultithreadProtected(true);

            let video_device: ID3D11VideoDevice = device.cast().context("o device não decodifica vídeo")?;
            let video_context: ID3D11VideoContext = context.cast().context("o contexto não decodifica vídeo")?;
            let mut token = 0_u32;
            let mut manager: Option<IMFDXGIDeviceManager> = None;

            MFCreateDXGIDeviceManager(&mut token, &mut manager).context("sem gerente de device")?;

            let manager = manager.ok_or_else(|| anyhow!("o Media Foundation não devolveu o gerente"))?;

            manager.ResetDevice(&device, token).context("o gerente recusou o device")?;

            Ok(Self { device, context, video_device, video_context, manager, converter: None })
        }
    }

    /// O quadro decodificado (`sample`, uma fatia de um array de texturas) em RGBA no `target`.
    unsafe fn read_into<'target>(&mut self, sample: &IMFSample, (visible, matrix): ((u32, u32), Matrix), target: impl FnOnce(u32, u32) -> &'target mut [u8]) -> Result<()> {
        unsafe {
            let buffer: IMFDXGIBuffer = sample.GetBufferByIndex(0).context("quadro sem buffer")?.cast().context("o quadro não está na placa")?;
            let mut raw = std::ptr::null_mut();

            buffer.GetResource(&ID3D11Texture2D::IID, &mut raw).context("o quadro não deu a textura")?;

            let texture = ID3D11Texture2D::from_raw(raw);
            let slice = buffer.GetSubresourceIndex().context("o quadro não disse a fatia")?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();

            texture.GetDesc(&mut desc);

            let sizes = ((desc.Width, desc.Height), visible, matrix);

            if self.converter.as_ref().is_none_or(|converter| converter.sizes != sizes) {
                self.converter = Some(self.converter_for(sizes)?);
            }

            let converter = self.converter.as_ref().ok_or_else(|| anyhow!("sem conversão"))?;
            let view_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: slice } },
            };
            let mut input: Option<ID3D11VideoProcessorInputView> = None;

            self.video_device
                .CreateVideoProcessorInputView(&texture, &converter.enumerator, &view_desc, Some(&mut input))
                .context("a fatia do quadro não virou entrada")?;

            let (width, height) = visible;
            let shown = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            let stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: true.into(),
                pInputSurface: ManuallyDrop::new(input),
                ..Default::default()
            };

            // Só o que se vê: as linhas de enchimento do fim da textura ficam de fora.
            self.video_context.VideoProcessorSetStreamSourceRect(&converter.processor, 0, true, Some(&shown));

            let blit = self.video_context.VideoProcessorBlt(&converter.processor, &converter.output_view, 0, std::slice::from_ref(&stream));

            drop(ManuallyDrop::into_inner(stream.pInputSurface));
            blit.context("a placa não converteu o quadro")?;
            self.context.CopyResource(&converter.staging, &converter.output);

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();

            self.context
                .Map(&converter.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .context("a imagem convertida não abriu para leitura")?;

            let rgba = target(width, height);
            let row = width as usize * 4;
            let copied = if rgba.len() == row * height as usize {
                let pitch = mapped.RowPitch as usize;
                let source = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), pitch * (height as usize - 1) + row);

                for (line, from) in rgba.chunks_exact_mut(row).zip(source.chunks(pitch)) {
                    line.copy_from_slice(&from[..row]);

                    if converter.swapped {
                        for pixel in line.as_chunks_mut::<4>().0 {
                            pixel.swap(0, 2);
                        }
                    }
                }

                Ok(())
            } else {
                Err(anyhow!("imagem de {} bytes para {width}x{height}", rgba.len()))
            };

            // Solta antes de qualquer erro subir: mapeada, ela trava a próxima cópia da placa.
            self.context.Unmap(&converter.staging, 0);

            copied
        }
    }

    unsafe fn converter_for(&self, (input, output, matrix): ((u32, u32), (u32, u32), Matrix)) -> Result<Converter> {
        unsafe {
            let rate = DXGI_RATIONAL { Numerator: 60, Denominator: 1 };
            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: rate,
                InputWidth: input.0,
                InputHeight: input.1,
                OutputFrameRate: rate,
                OutputWidth: output.0,
                OutputHeight: output.1,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            let enumerator = self.video_device.CreateVideoProcessorEnumerator(&content).context("sem conversão de vídeo na placa")?;
            let processor = self.video_device.CreateVideoProcessor(&enumerator, 0).context("o conversor não abriu")?;
            let supports = |format: DXGI_FORMAT| {
                enumerator
                    .CheckVideoProcessorFormat(format)
                    .is_ok_and(|support| support & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32 != 0)
            };
            let (format, swapped) = if supports(DXGI_FORMAT_R8G8B8A8_UNORM) {
                (DXGI_FORMAT_R8G8B8A8_UNORM, false)
            } else {
                (DXGI_FORMAT_B8G8R8A8_UNORM, true)
            };

            self.video_context.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);

            // O inverso do encoder: entra NV12 limitado (16–235) na matriz que o vídeo declarou —
            // BT.709 nos nossos encoders, BT.601 em vídeo SD —, sai RGB cheio (0–255). Com a matriz
            // errada o amarelo puxa para o verde.
            match self.video_context.cast::<ID3D11VideoContext1>() {
                Ok(context) => {
                    let stream = match matrix {
                        Matrix::Bt709 => DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                        Matrix::Bt601 => DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P601,
                    };

                    context.VideoProcessorSetStreamColorSpace1(&processor, 0, stream);
                    context.VideoProcessorSetOutputColorSpace1(&processor, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
                }
                Err(_) => {
                    let mut stream = color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235);

                    // O bit da matriz (o terceiro) desligado é BT.601.
                    if matrix == Matrix::Bt601 {
                        stream._bitfield &= !(1 << 2);
                    }

                    self.video_context.VideoProcessorSetStreamColorSpace(&processor, 0, &stream);
                    self.video_context.VideoProcessorSetOutputColorSpace(&processor, &color_space(D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_0_255));
                }
            }

            let texture = |staging: bool| -> Result<ID3D11Texture2D> {
                let descriptor = D3D11_TEXTURE2D_DESC {
                    Width: output.0,
                    Height: output.1,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: if staging { D3D11_USAGE_STAGING } else { D3D11_USAGE_DEFAULT },
                    BindFlags: if staging { 0 } else { D3D11_BIND_RENDER_TARGET.0 as u32 },
                    CPUAccessFlags: if staging { D3D11_CPU_ACCESS_READ.0 as u32 } else { 0 },
                    MiscFlags: 0,
                };
                let mut created: Option<ID3D11Texture2D> = None;

                self.device.CreateTexture2D(&descriptor, None, Some(&mut created)).context("sem textura para a imagem")?;

                created.ok_or_else(|| anyhow!("a textura da imagem não foi criada"))
            };
            let output_texture = texture(false)?;
            let staging = texture(true)?;
            let view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC { ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D, ..Default::default() };
            let mut output_view: Option<ID3D11VideoProcessorOutputView> = None;

            self.video_device
                .CreateVideoProcessorOutputView(&output_texture, &enumerator, &view_desc, Some(&mut output_view))
                .context("a imagem não virou saída do conversor")?;

            Ok(Converter {
                sizes: (input, output, matrix),
                enumerator,
                processor,
                output: output_texture,
                output_view: output_view.ok_or_else(|| anyhow!("sem saída do conversor"))?,
                staging,
                swapped,
            })
        }
    }
}

pub struct H264Decoder {
    transform: IMFTransform,
    layout: Layout,
    /// A amostra de saída da vez anterior, para a próxima: alocar uma do tamanho do quadro a
    /// cada chamada eram vários MB por quadro (12 em 4K), em PC fraco disputando memória com o
    /// jogo. Só existe quando quem aloca a saída é este lado, e cai na troca de formato.
    spare: Option<IMFSample>,
    /// A placa, quando ela decodifica. `None` é o caminho da CPU.
    gpu: Option<Gpu>,
}

// O MFT de software é free-threaded, e o decodificador só é usado por uma thread por vez:
// a da fila de mídia de quem assiste.
unsafe impl Send for H264Decoder {}

impl H264Decoder {
    pub fn new() -> Result<Self> {
        Self::open(gpu_allowed())
    }

    fn open(on_gpu: bool) -> Result<Self> {
        unsafe {
            start_media_foundation().map_err(|failure| anyhow!("{failure}"))?;

            let gpu = on_gpu
                .then(|| Gpu::open().inspect_err(|failure| tracing::info!(%failure, "decodificador: a placa não abriu, vai na CPU")).ok())
                .flatten();
            let (transform, gpu) = match gpu {
                Some(gpu) => match open_decoder(Some(&gpu.manager)) {
                    Ok(transform) => (transform, Some(gpu)),
                    Err(failure) => {
                        tracing::info!(%failure, "decodificador: nenhum MFT decodifica na placa, vai na CPU");

                        (open_decoder(None)?, None)
                    }
                },
                None => (open_decoder(None)?, None),
            };

            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .context("o decodificador recusou o começo do streaming")?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .context("o decodificador recusou o começo do vídeo")?;

            let mut decoder = Self {
                transform,
                layout: Layout::default(),
                spare: None,
                gpu,
            };

            decoder.choose_output()?;

            Ok(decoder)
        }
    }

    /// Entrega um quadro Annex-B e devolve a imagem que ficou pronta, se ficou. Com a baixa
    /// latência ligada o MFT devolve o próprio quadro na hora; o primeiro às vezes só sai junto
    /// com o segundo, e aí vale o mais novo.
    pub fn decode(&mut self, annex_b: &[u8], timestamp: u32) -> Result<Option<DecodedFrame>> {
        let mut frame = None::<DecodedFrame>;
        let drawn = self.decode_into(annex_b, timestamp, |width, height| {
            &mut frame
                .insert(DecodedFrame { width, height, rgba: vec![0; width as usize * height as usize * 4] })
                .rgba
        })?;

        Ok(frame.filter(|_| drawn))
    }

    /// O mesmo, escrevendo a imagem em RGBA direto no buffer que `target` der para o tamanho
    /// dela — o da imagem da interface: sem um `Vec` no meio e sem a cópia dele para lá, que
    /// eram ~0,7 GB/s numa tela 1080p60. Diz se saiu imagem.
    pub fn decode_into<'target>(
        &mut self,
        annex_b: &[u8],
        timestamp: u32,
        target: impl FnOnce(u32, u32) -> &'target mut [u8],
    ) -> Result<bool> {
        let Some(sample) = (unsafe { self.feed(annex_b, timestamp)? }) else {
            return Ok(false);
        };
        let filled = unsafe { self.read_into(&sample, target) };

        self.recycle(sample);

        filled.map(|()| true)
    }

    /// Decodifica sem virar imagem. Todo quadro P tem de passar pelo decodificador para o
    /// seguinte sair certo, mas converter um quadro que outro mais novo vai substituir antes de
    /// a janela desenhar é trabalho jogado fora — era o que deixava quem assiste duas telas
    /// 1080p60 quatro segundos atrás.
    pub fn skip(&mut self, annex_b: &[u8], timestamp: u32) -> Result<()> {
        if let Some(sample) = unsafe { self.feed(annex_b, timestamp)? } {
            self.recycle(sample);
        }

        Ok(())
    }

    /// A amostra que saiu por último, já com o quadro dentro.
    unsafe fn feed(&mut self, annex_b: &[u8], timestamp: u32) -> Result<Option<IMFSample>> {
        unsafe {
            let sample = input_sample(annex_b, timestamp)?;
            let mut last = None;

            if let Err(failure) = self.transform.ProcessInput(0, &sample, 0) {
                if failure.code() != MF_E_NOTACCEPTING {
                    return Err(anyhow!(failure).context("o decodificador recusou o quadro"));
                }

                // Cheio: o que estava pronto sai primeiro, e aí o quadro entra.
                self.drain(&mut last)?;
                self.transform
                    .ProcessInput(0, &sample, 0)
                    .context("o decodificador recusou o quadro depois de esvaziar")?;
            }

            self.drain(&mut last)?;

            Ok(last)
        }
    }

    unsafe fn drain(&mut self, last: &mut Option<IMFSample>) -> Result<()> {
        let mut changes = 0;

        loop {
            match unsafe { self.next_output()? } {
                Output::Frame(sample) => {
                    if let Some(older) = last.replace(sample) {
                        self.recycle(older);
                    }
                }
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
            let allocating = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 == 0;

            if allocating {
                let sample = match self.spare.take() {
                    Some(sample) => sample,
                    None => {
                        let size = info.cbSize.max(self.layout.stride * self.layout.rows * 3 / 2).max(1);
                        let buffer = MFCreateMemoryBuffer(size).context("sem memória para o quadro")?;
                        let sample = MFCreateSample().context("sem amostra para o quadro")?;

                        sample.AddBuffer(&buffer).context("a amostra recusou o buffer")?;

                        sample
                    }
                };

                output[0].pSample = ManuallyDrop::new(Some(sample));
            }

            let result = self.transform.ProcessOutput(0, &mut output, &mut status);
            let sample = ManuallyDrop::take(&mut output[0].pSample);

            drop(ManuallyDrop::take(&mut output[0].pEvents));

            match result {
                Ok(()) => {}
                Err(failure) if failure.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                    if allocating {
                        self.spare = sample;
                    }

                    return Ok(Output::Empty);
                }
                // O tamanho novo pode não caber na amostra guardada.
                Err(failure) if failure.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    self.spare = None;

                    return Ok(Output::StreamChanged);
                }
                Err(failure) => return Err(anyhow!(failure).context("o decodificador falhou no quadro")),
            }

            Ok(Output::Frame(sample.ok_or_else(|| anyhow!("o decodificador não devolveu amostra"))?))
        }
    }

    /// Guarda a amostra para a próxima saída, se é este lado quem aloca.
    fn recycle(&mut self, sample: IMFSample) {
        let allocating = unsafe { self.transform.GetOutputStreamInfo(0) }
            .is_ok_and(|info| info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 == 0);

        if allocating && self.spare.is_none() {
            self.spare = Some(sample);
        }
    }

    unsafe fn read_into<'target>(&mut self, sample: &IMFSample, target: impl FnOnce(u32, u32) -> &'target mut [u8]) -> Result<()> {
        unsafe {
            if let Some(gpu) = self.gpu.as_mut() {
                let read = gpu.read_into(sample, ((self.layout.width, self.layout.height), self.layout.matrix), target);

                // Uma falha da placa no meio (driver que reiniciou, device removido) vale para o
                // resto do processo: o próximo decodificador — o que nasce no próximo quadro-chave
                // — abre na CPU em vez de falhar do mesmo jeito.
                if read.is_err() {
                    GPU_FAILED.store(true, Ordering::Relaxed);
                }

                return read;
            }

            let buffer = sample
                .ConvertToContiguousBuffer()
                .context("o quadro decodificado não virou um buffer só")?;
            let mut start = std::ptr::null_mut();
            let mut size = 0_u32;

            buffer.Lock(&mut start, None, Some(&mut size)).context("o quadro não abriu para leitura")?;

            let nv12 = std::slice::from_raw_parts(start, size as usize);
            let converted = nv12_to_rgba(nv12, self.layout, target(self.layout.width, self.layout.height));

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
    Frame(IMFSample),
    Empty,
    StreamChanged,
}

/// O primeiro decodificador de H.264 que aceitar entrar. Só os síncronos: o assíncrono
/// pede eventos, e o da Microsoft — que todo Windows tem — é síncrono.
unsafe fn open_decoder(manager: Option<&IMFDXGIDeviceManager>) -> Result<IMFTransform> {
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
            match try_decoder(&activate, manager) {
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

unsafe fn try_decoder(activate: &IMFActivate, manager: Option<&IMFDXGIDeviceManager>) -> Result<IMFTransform> {
    unsafe {
        let transform: IMFTransform = activate.ActivateObject().context("o MFT não ativou")?;

        // O gerente antes dos tipos: é com ele que o MFT decide em que memória decodifica.
        if let Some(manager) = manager {
            let aware = transform.GetAttributes().ok().and_then(|attributes| attributes.GetUINT32(&MF_SA_D3D11_AWARE).ok());

            if aware != Some(1) {
                return Err(anyhow!("o MFT não decodifica na placa"));
            }

            transform
                .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
                .context("o MFT recusou o gerente do Direct3D")?;
        }

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

/// NV12 de faixa limitada para RGBA, na matriz que o vídeo declarou, escrito em `rgba` — que
/// tem de ter o tamanho exato da imagem. Conta inteira em ponto fixo de 8 bits: cabe num `i32` e
/// não tem divisão nem arredondamento por pixel. RGBA, e não RGB, porque é o que a imagem do
/// Slint guarda e a placa recebe sem conversão na hora de desenhar.
fn nv12_to_rgba(nv12: &[u8], layout: Layout, rgba: &mut [u8]) -> Result<()> {
    let Layout { width, height, stride, rows, matrix } = layout;
    let (luma_gain, red_v, green_u, green_v, blue_u) = matrix.coefficients();
    let (width, height, stride, rows) = (width as usize, height as usize, stride as usize, rows as usize);
    let chroma_start = stride * rows;
    let needed = chroma_start + stride * height.div_ceil(2);

    if width == 0 || height == 0 || nv12.len() < needed || rgba.len() != width * height * 4 {
        return Err(anyhow!(
            "quadro de {} bytes não cabe em {width}x{height} com linha de {stride}, para {} bytes de imagem",
            nv12.len(),
            rgba.len()
        ));
    }

    let line = |(row, pixels): (usize, &mut [u8])| {
        let luma = &nv12[row * stride..row * stride + width];
        let chroma = &nv12[chroma_start + (row / 2) * stride..];

        for (column, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let y = (i32::from(luma[column]) - 16) * luma_gain;
            let u = i32::from(chroma[column & !1]) - 128;
            let v = i32::from(chroma[(column & !1) + 1]) - 128;

            pixel[0] = clamp((y + red_v * v + 128) >> 8);
            pixel[1] = clamp((y - green_u * u - green_v * v + 128) >> 8);
            pixel[2] = clamp((y + blue_u * u + 128) >> 8);
            pixel[3] = 255;
        }
    };

    // As linhas não dependem umas das outras. Sem o pool, numa thread só: mais lento, mas a
    // imagem sai.
    match conversion_pool() {
        Some(pool) => pool.install(|| rgba.par_chunks_exact_mut(width * 4).enumerate().for_each(line)),
        None => rgba.chunks_exact_mut(width * 4).enumerate().for_each(line),
    }

    Ok(())
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

    /// É o que quem assiste faz quando fica para trás: decodifica sem converter e só o mais
    /// novo vira imagem. O último tem de sair igual ao de quem converteu todos.
    #[test]
    fn frames_skipped_still_feed_the_ones_after_them() {
        let units = access_units(FIXTURE);
        let (mut every, mut skipping) = (H264Decoder::new().expect("abriu"), H264Decoder::new().expect("abriu"));
        let mut converted = Vec::new();

        for (index, unit) in units.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let timestamp = index as u32 * 3_000;

            converted.extend(every.decode(unit, timestamp).expect("o quadro decodificou"));

            if index + 1 < units.len() {
                skipping.skip(unit, timestamp).expect("o quadro passou sem converter");
            } else {
                let last = skipping.decode(unit, timestamp).expect("o último decodificou");

                assert!(last.is_some(), "o pulo segurou o último quadro");
                assert_eq!(last.as_ref(), converted.last(), "o último saiu diferente depois dos pulos");
            }
        }
    }

    /// Os dois caminhos — a placa, quando esta máquina tem, e a CPU — saem com as mesmas cores
    /// no mesmo lugar.
    #[test]
    fn every_frame_of_a_real_stream_comes_out_as_pixels() {
        for on_gpu in [true, false] {
            every_frame_comes_out_as_pixels(on_gpu);
        }
    }

    fn every_frame_comes_out_as_pixels(on_gpu: bool) {
        let mut decoder = H264Decoder::open(on_gpu).expect("o decodificador abriu");
        let mut frames = Vec::new();

        for (index, unit) in access_units(FIXTURE).into_iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let timestamp = index as u32 * 3_000;

            frames.extend(decoder.decode(unit, timestamp).expect("o quadro decodificou"));
        }

        assert_eq!(frames.len(), 6, "o MFT segurou quadro: saíram {}", frames.len());

        for frame in &frames {
            assert_eq!((frame.width, frame.height), (320, 240));
            assert_eq!(frame.rgba.len(), 320 * 240 * 4);
        }

        // As sete barras do `smpte` do GStreamer, da esquerda para a direita. Cor certa em
        // cada uma prova a matriz, e cada uma no lugar certo prova a geometria do buffer.
        let bars = [
            [255, 255, 255], [255, 255, 0], [0, 255, 255], [0, 255, 0],
            [255, 0, 255], [255, 0, 0], [0, 0, 255],
        ];

        for (index, expected) in bars.iter().enumerate() {
            let x = 320 * (2 * index + 1) / 14;
            let pixel = &frames[0].rgba[(20 * 320 + x) * 4..(20 * 320 + x) * 4 + 3];
            let far = pixel.iter().zip(expected).any(|(&got, &want)| (i32::from(got) - want).abs() > 12);

            assert!(!far, "a barra {index} saiu {pixel:?}, esperava {expected:?} (placa: {})", decoder.gpu.is_some());
        }
    }

    /// Ida e volta de verdade, na placa desta máquina: 1080p saído do encoder de hardware — cuja
    /// textura tem 1088 linhas, e as 8 de baixo não podem aparecer — decodificado pelos dois
    /// caminhos, com o tempo de cada um. Precisa de placa, por isso fica de fora do `cargo test`:
    ///
    /// `cargo test -p media --lib a_1080p_stream -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn a_1080p_stream_from_the_hardware_encoder_decodes_on_both_paths() {
        use ::windows::Win32::Graphics::Direct3D11::{D3D11_BIND_SHADER_RESOURCE, D3D11_USAGE_DEFAULT};

        let (width, height) = (1920_u32, 1080_u32);
        let (device, context) = unsafe { create_device() }.expect("device");
        let mut texture = None;

        unsafe {
            device
                .CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: width,
                        Height: height,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                        Usage: D3D11_USAGE_DEFAULT,
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        CPUAccessFlags: 0,
                        MiscFlags: 0,
                    },
                    None,
                    Some(&mut texture),
                )
                .expect("textura");
        }

        let surface = crate::GpuSurface { texture: texture.expect("textura"), device, context };
        let mut encoder = crate::PlatformEncoder::new(&crate::EncoderConfig::new(capture::Quality::Hd1080, 60, (width, height))).expect("encoder");
        let mut stream = Vec::new();

        for index in 0..180_u32 {
            // Um degradê que anda: muda todo quadro, como um jogo, e tem cor para conferir.
            let pixels: Vec<u8> = (0..width * height)
                .flat_map(|pixel| {
                    let shade = ((pixel % width + index * 8) % 256) as u8;

                    [shade, 64, 255 - shade, 255]
                })
                .collect();

            unsafe { surface.context.UpdateSubresource(&surface.texture, 0, None, pixels.as_ptr().cast(), width * 4, 0) };

            if let Ok(frame) = encoder.encode(&surface, u64::from(index) * 16_666_667) {
                stream.push(frame);
            }
        }

        assert!(stream.len() > 150, "o encoder soltou {} quadros", stream.len());

        let mut last = Vec::new();

        for on_gpu in [true, false] {
            let mut decoder = H264Decoder::open(on_gpu).expect("decodificador");
            let begun = std::time::Instant::now();
            let mut image = None;

            for (index, frame) in stream.iter().enumerate() {
                #[allow(clippy::cast_possible_truncation)]
                let timestamp = index as u32 * 1_500;

                if let Some(decoded) = decoder.decode(&frame.data, timestamp).expect("decodificou") {
                    image = Some(decoded);
                }
            }

            let spent = begun.elapsed();
            let image = image.expect("saiu imagem");

            println!(
                "{}: {:.2} ms por quadro decodificado e convertido ({} quadros)",
                if decoder.gpu.is_some() { "placa" } else { "CPU" },
                spent.as_secs_f64() * 1000.0 / stream.len() as f64,
                stream.len()
            );
            assert_eq!((image.width, image.height), (width, height), "o enchimento entrou na imagem");
            last.push(image.rgba);
        }

        // Os dois caminhos chegam à mesma imagem, a menos do arredondamento de cada conversão.
        let differing = last[0].iter().zip(&last[1]).filter(|(gpu, cpu)| (i32::from(**gpu) - i32::from(**cpu)).abs() > 16).count();

        assert!(differing < last[0].len() / 100, "{differing} canais diferentes entre placa e CPU");
    }

    fn convert(nv12: &[u8], layout: Layout) -> Result<Vec<u8>> {
        let mut rgba = vec![0; layout.width as usize * layout.height as usize * 4];

        nv12_to_rgba(nv12, layout, &mut rgba).map(|()| rgba)
    }

    #[test]
    fn nv12_white_black_and_red_become_the_right_rgba() {
        // Um bloco 2x2 tem um par de croma só, então cada caso pinta o bloco inteiro.
        let white = Layout { width: 2, height: 2, stride: 2, rows: 2, matrix: Matrix::Bt709 };
        let rgba = convert(&[235, 235, 235, 235, 128, 128], white).unwrap();

        assert!(rgba.iter().all(|&channel| channel >= 254), "{rgba:?}");

        let rgba = convert(&[16, 16, 16, 16, 128, 128], white).unwrap();

        assert!(rgba.chunks(4).all(|pixel| pixel[..3].iter().all(|&channel| channel <= 1) && pixel[3] == 255), "{rgba:?}");

        // Vermelho puro em BT.709 limitado: Y 63, U 102, V 240.
        let rgba = convert(&[63, 63, 63, 63, 102, 240], white).unwrap();
        let pixel = &rgba[..4];

        assert!(pixel[0] >= 250 && pixel[1] <= 5 && pixel[2] <= 5 && pixel[3] == 255, "{pixel:?}");
    }

    #[test]
    fn the_padding_rows_and_columns_are_left_out() {
        // Um vídeo 2x2 dentro de um buffer de 4 colunas e 4 linhas, como o MFT alinha.
        let layout = Layout { width: 2, height: 2, stride: 4, rows: 4, matrix: Matrix::Bt709 };
        let mut nv12 = vec![0_u8; 4 * 4 + 4 * 2];

        nv12[..2].copy_from_slice(&[235, 235]);
        nv12[4..6].copy_from_slice(&[235, 235]);
        nv12[16..18].copy_from_slice(&[128, 128]);

        let rgba = convert(&nv12, layout).unwrap();

        assert_eq!(rgba.len(), 16);
        assert!(rgba.iter().all(|&channel| channel >= 254), "{rgba:?}");
    }

    #[test]
    fn a_short_buffer_or_a_wrong_image_is_refused_instead_of_read_past_the_end() {
        let layout = Layout { width: 4, height: 4, stride: 4, rows: 4, matrix: Matrix::Bt709 };

        assert!(convert(&[0; 10], layout).is_err());
        assert!(nv12_to_rgba(&[0; 24], layout, &mut [0; 10]).is_err(), "imagem do tamanho errado");
    }
}
