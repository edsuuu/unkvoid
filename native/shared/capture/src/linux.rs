//! Captura no Linux: X11 ou Wayland pelo GStreamer, já codificada.
//!
//! O vídeo roda dentro do processo, pelo `gstreamer-rs`: `ximagesrc` (X11) ou `pipewiresrc`
//! (Wayland) lê a tela, o encoder da placa que abrir (`LinuxCapturer::video_encoder`) ou o
//! `x264enc` comprime, e o H.264 (Annex-B) sai num `appsink`. Dentro, e não num `gst-launch`,
//! porque é o único jeito de falar com o encoder no ar: o PLI de quem assiste vira keyframe e
//! a perda vira taxa menor (`EncoderControl`). O som, a prévia e a sondagem dos encoders
//! continuam em `gst-launch-1.0` — nada disso precisa ouvir pedido, e a sondagem de um driver
//! que pendura fica isolada num processo que dá para matar.
//!
//! O que sai daqui NÃO é buffer de GPU: é o quadro pronto, e `PlatformEncoder` no
//! Linux só o repassa. É o jeito de encaixar no fluxo dos outros sistemas sem mexer
//! no `broadcast.rs`.
//!
//! No Wayland o app não enxerga a tela: o `ximagesrc` só vê o XWayland, preto ou vazio.
//! Quem mostra é o portal `org.freedesktop.portal.ScreenCast`: o seletor é o do próprio
//! sistema (monitor ou janela), e o que ele devolve é um nó do PipeWire e um fd para lê-lo.
//! Por isso lá a lista de telas tem um item só e a de janelas nenhum.
//!
//! ponytail: no X11 continua sem lista de janelas; o `ximagesrc xid=` a traria.

use std::io::{BufRead, BufReader, Read};
use std::os::fd::{AsRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{ResponseError, Session};
use ashpd::enumflags2::BitFlags;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::linux_audio::{self, SharedSink};
use crate::{
    AudioChunk, CaptureConfig, CaptureError, CaptureEvent, CaptureSource, Display, Quality,
    VideoFrame, Window,
};

/// Um quadro já em H.264 Annex-B, com SPS/PPS na frente de cada keyframe.
#[derive(Clone)]
pub struct EncodedVideo {
    pub data: Vec<u8>,
    pub keyframe: bool,
    /// O encoder que o comprimiu. Vai junto com o quadro para o `PlatformEncoder` do `media`,
    /// que é quem recebe o pedido de keyframe e a taxa nova, achá-lo sem o núcleo mudar.
    pub encoder: Option<EncoderControl>,
}

/// O encoder de um pipeline no ar.
#[derive(Clone)]
pub struct EncoderControl {
    encoder: gst::Element,
    sink: gst::Element,
    /// A taxa com que o pipeline abriu, em bit/s.
    launched: u32,
}

impl EncoderControl {
    pub fn launched_bitrate(&self) -> u32 {
        self.launched
    }

    /// Um keyframe agora, com SPS/PPS na frente. O evento sobe do `appsink` pelo `h264parse`
    /// até o encoder — o mesmo que o `GstVideoEncoder` de cada um deles entende.
    pub fn request_keyframe(&self) {
        let keyframe = gst::Structure::builder("GstForceKeyUnit").field("all-headers", true).build();

        self.sink.send_event(gst::event::CustomUpstream::new(keyframe));
    }

    /// A taxa nova com o pipeline tocando, em bit/s. `false` quando este encoder não aceita
    /// a troca no ar: quem chama desiste, e a taxa fica a de abertura.
    pub fn set_bitrate(&self, bits_per_second: u32) -> bool {
        let Some(property) = self.encoder.find_property("bitrate") else {
            return false;
        };

        if !property.flags().contains(gst::PARAM_FLAG_MUTABLE_PLAYING) || property.value_type() != u32::static_type() {
            return false;
        }

        self.encoder.set_property("bitrate", (bits_per_second / 1000).max(1));

        true
    }
}

/// 48 kHz estéreo em `f32`, 20 ms por bloco — o que o `AudioEncoder` espera.
const AUDIO_BLOCK_BYTES: usize = 48_000 / 50 * 2 * 4;

pub struct LinuxCapturer {
    video: Option<VideoPipeline>,
    audio: Option<Child>,
    /// O sink que filtra o som por app; cai no `stop`, e o som volta à saída padrão.
    shared_sink: Option<SharedSink>,
    frames: Arc<AtomicU64>,
    audio_chunks: Arc<AtomicU64>,
    /// A última linha de erro do gst de vídeo. É o que aparece no app quando a captura
    /// não gera quadro nenhum — sem isto o diagnóstico culpava a rede.
    error: Arc<Mutex<Option<String>>>,
    /// A sessão do portal, no Wayland. Nada a lê: o capturador só a mantém viva, e ela
    /// fecha quando o último que a segura some — não no `stop`, porque trocar a qualidade
    /// para este capturador e abre outro na MESMA sessão.
    _portal: Option<Arc<PortalSession>>,
}

impl LinuxCapturer {
    pub fn preview(source: CaptureSource) -> Result<Vec<u8>, CaptureError> {
        // No portal não há o que mostrar antes de a pessoa escolher no seletor do sistema.
        if backend() == Backend::Portal || std::env::var_os("DISPLAY").is_none() {
            return Ok(Vec::new());
        }

        let pipeline = format!(
            "ximagesrc use-damage=false num-buffers=1 {} ! videoconvert ! videoscale              ! video/x-raw,width=320,pixel-aspect-ratio=1/1 ! jpegenc ! fdsink fd=1",
            region(source).map(|monitor| monitor.area()).unwrap_or_default()
        );

        let output = Command::new("timeout")
            .args(["3", "gst-launch-1.0", "-q"])
            .args(pipeline.split_whitespace())
            .stderr(Stdio::null())
            .output();

        Ok(output.map(|output| output.stdout).unwrap_or_default())
    }

    /// Um item por monitor, o principal primeiro. Sem `xrandr` fica a tela do X
    /// inteira, que com dois monitores é os dois lado a lado.
    ///
    /// No portal é um item só e sem tamanho: a lista de verdade é a do seletor do
    /// sistema, que só abre no `prepare`.
    pub fn displays() -> Result<Vec<Display>, CaptureError> {
        if backend() == Backend::Portal {
            return Ok(vec![Display { id: 1, width: 0, height: 0 }]);
        }

        if std::env::var_os("DISPLAY").is_none() {
            return Ok(Vec::new());
        }

        let monitors = monitors();

        if monitors.is_empty() {
            let (width, height) = screen_size().unwrap_or((0, 0));

            return Ok(vec![Display { id: 1, width, height }]);
        }

        Ok(monitors
            .iter()
            .enumerate()
            .map(|(index, monitor)| Display {
                id: index as u32 + 1,
                width: monitor.width,
                height: monitor.height,
            })
            .collect())
    }

    pub fn windows() -> Result<Vec<Window>, CaptureError> {
        Ok(Vec::new())
    }

    /// As câmeras: `(caminho, nome)` de cada `/dev/video*`, pelo nome que o driver dá.
    ///
    /// A uvcvideo cria um nó de metadados ao lado do de captura, com o mesmo nome e o
    /// número seguinte; o de captura é o menor. Por isso a ordem é numérica e o nome
    /// repetido fica com o primeiro índice.
    pub fn cameras() -> Vec<(String, String)> {
        if std::env::var_os("UNKVOID_CAMERA_SOURCE").is_some_and(|source| !source.is_empty()) {
            return vec![("/dev/video0".to_string(), "UNKVOID_CAMERA_SOURCE".to_string())];
        }

        let found = std::fs::read_dir("/sys/class/video4linux")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let node = entry.file_name().into_string().ok()?;
                let index: u32 = node.strip_prefix("video")?.parse().ok()?;
                let name = std::fs::read_to_string(entry.path().join("name")).ok()?;

                Some((index, name.trim().to_string()))
            });

        dedupe_cameras(found)
    }

    /// O tamanho da origem, para a altura da saída seguir a proporção dela. No portal é
    /// o do que a pessoa escolheu no `prepare`.
    pub fn source_size(source: CaptureSource) -> Result<(u32, u32), CaptureError> {
        Ok(match source {
            CaptureSource::Camera(_) => CAMERA_SIZE,
            CaptureSource::Microphone => (0, 0),
            _ if backend() == Backend::Portal => portal_session(false)?.size,
            _ => region(source)
                .map(|monitor| (monitor.width, monitor.height))
                .or_else(screen_size)
                .ok_or(CaptureError::NoDisplay)?,
        })
    }

    /// Descobre o que o GStreamer desta máquina sabe, fora de qualquer cadeado: a
    /// primeira pergunta abre um `gst-inspect`, e as seguintes lêem o cache.
    pub fn warm_up() {
        has_webrtcdsp();
        Self::video_encoder();
    }

    /// O encoder de H.264 desta máquina: o primeiro de `HARDWARE_H264_ENCODERS` que abre de
    /// verdade, ou o `x264enc`.
    ///
    /// Aparecer no `gst-inspect` não basta: o plugin da NVIDIA ou do VA-API vem instalado
    /// em máquina sem a placa, e só abrir o device diz. Cada candidato codifica um quadro
    /// de teste com o mesmo trecho de pipeline que a transmissão vai usar, uma vez por
    /// processo.
    pub fn video_encoder() -> &'static str {
        static CHOSEN: OnceLock<&'static str> = OnceLock::new();

        CHOSEN.get_or_init(|| {
            let forced_cpu = std::env::var("UNKVOID_ENCODER").is_ok_and(|value| value == "cpu");
            let chosen = HARDWARE_H264_ENCODERS
                .into_iter()
                .find(|element| !forced_cpu && encoder_opens(element))
                .unwrap_or("x264enc");

            tracing::info!(encoder = chosen, "captura: encoder de H.264 escolhido");

            chosen
        })
    }

    pub fn start<F>(config: &CaptureConfig, on_event: F) -> Result<Self, CaptureError>
    where
        F: Fn(CaptureEvent) + Send + Sync + 'static,
    {
        let on_event: Arc<dyn Fn(CaptureEvent) + Send + Sync> = Arc::new(on_event);
        let frames = Arc::new(AtomicU64::new(0));
        let audio_chunks = Arc::new(AtomicU64::new(0));
        let error = Arc::new(Mutex::new(None));

        // Microfone: só áudio, com a mesma forma da tela para o app não saber a diferença.
        if config.source == CaptureSource::Microphone {
            let mut audio = launch(&microphone_pipeline(), Stdio::null())?;

            watch_stderr(&mut audio, Arc::clone(&error));
            read_audio(&mut audio, Arc::clone(&audio_chunks), on_event);

            return Ok(Self { video: None, audio: Some(audio), shared_sink: None, frames, audio_chunks, error, _portal: None });
        }

        if let CaptureSource::Camera(index) = config.source {
            let video = VideoPipeline::start(
                &camera_pipeline(index),
                (None, CAMERA_SIZE, CAMERA_BITRATE * 1000),
                (Arc::clone(&frames), Arc::clone(&error)),
                on_event,
            )?;

            return Ok(Self { video: Some(video), audio: None, shared_sink: None, frames, audio_chunks, error, _portal: None });
        }

        let portal = match backend() {
            Backend::Portal => Some(portal_session(true)?),
            Backend::X11 if std::env::var_os("DISPLAY").is_none() => return Err(CaptureError::NoDisplay),
            Backend::X11 => None,
        };

        let source_size = match &portal {
            Some(session) => session.size,
            None => Self::source_size(config.source)?,
        };
        // Sem encoder na placa o x264 roda na CPU de quem também está jogando: 720p30, o mesmo
        // teto do `EncoderConfig::for_cpu` do Windows. Na resolução e no fps cheios ele tomava o
        // processador inteiro (`threads=0`) e mesmo assim não sustentava 1080p60 em PC fraco.
        let on_cpu = Self::video_encoder() == "x264enc";
        let quality = if on_cpu { Quality::Hd720 } else { config.quality };
        let (width, height) = quality.fit(source_size);
        let frame_rate = config.frame_rate.clamp(1, if on_cpu { 30 } else { 60 });

        let bitrate = match quality {
            Quality::Hd720 => 5_000,
            Quality::Hd1080 => 10_000,
            Quality::Qhd1440 => 20_000,
            Quality::Uhd2160 => 40_000,
        } * frame_rate
            / 60;

        // BT.709 fixo antes do x264: sem ele a colorimetria dependia da resolução escolhida,
        // e o VUI que o x264 escreve saía diferente do que o decodificador supõe. O keyframe
        // por segundo continua: é o que quem entra na sala espera no pior caso, se o PLI dele
        // se perder.
        let (format, encoder) = encoder_tail(Self::video_encoder(), frame_rate, bitrate);

        let (source, remote) = match &portal {
            Some(session) => {
                // O fd fica neste processo enquanto a captura roda. O `try_clone` o devolve com
                // `CLOEXEC`: sem isso o `gst-launch` do som, que nasce logo depois, herdaria o
                // acesso à tela (passo 7 do roteiro do Wayland).
                let remote = session
                    .remote()?
                    .try_clone()
                    .map_err(|failure| CaptureError::Platform(format!("o fd do PipeWire não duplicou ({failure})")))?;

                (portal_source(remote.as_raw_fd(), session.node, frame_rate), Some(remote))
            }
            None => (x11_source(config.show_cursor, region(config.source), window_of(config.source)), None),
        };

        let video = VideoPipeline::start(
            &screen_pipeline(&source, frame_rate, (width, height), format, &encoder),
            (remote, (width, height), bitrate * 1000),
            (Arc::clone(&frames), Arc::clone(&error)),
            Arc::clone(&on_event),
        )?;

        let shared_sink = if config.capture_audio {
            SharedSink::open(config.mute_listed_apps)
                .inspect_err(|error| tracing::warn!(error = %error, "captura: sem filtro por app, o som do sistema vai inteiro"))
                .ok()
        } else {
            None
        };
        let monitor = if shared_sink.is_some() { linux_audio::MONITOR } else { "@DEFAULT_MONITOR@" };

        let audio = if config.capture_audio {
            match launch(&format!("pulsesrc device={monitor} ! {AUDIO_TAIL}"), Stdio::null()) {
                Ok(mut child) => {
                    // O stderr vai para o log: sem servidor de som o gst sai na hora, e só a
                    // linha dele diz por quê.
                    watch_stderr(&mut child, Arc::clone(&error));
                    read_audio(&mut child, Arc::clone(&audio_chunks), on_event);

                    Some(child)
                }
                Err(error) => {
                    tracing::warn!(error = %error, "captura: áudio do sistema indisponível, vai sem som");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self { video: Some(video), audio, shared_sink, frames, audio_chunks, error, _portal: portal })
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|slot| slot.clone())
    }

    pub fn frames_captured(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn audio_chunks_captured(&self) -> u64 {
        self.audio_chunks.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) -> Result<(), CaptureError> {
        self.video = None;

        if let Some(child) = self.audio.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }

        self.shared_sink = None;

        Ok(())
    }
}

impl Drop for LinuxCapturer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// De onde a tela vem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    /// `ximagesrc`: o app enxerga a tela e lista os monitores.
    X11,
    /// `pipewiresrc` pelo portal: quem escolhe é o seletor do próprio sistema.
    Portal,
}

/// Sessão Wayland vai pelo portal. `UNKVOID_CAPTURE=x11|portal` força um dos dois: é o
/// botão para calibrar numa máquina de verdade (GNOME e KDE têm portal também no X11, e
/// um compositor sem portal ainda tem o XWayland).
fn backend_for(forced: Option<&str>, session_type: Option<&str>, wayland_display: Option<&str>) -> Backend {
    match forced {
        Some("x11") => Backend::X11,
        Some("portal") => Backend::Portal,
        _ if session_type == Some("wayland") || wayland_display.is_some_and(|name| ! name.is_empty()) => {
            Backend::Portal
        }
        _ => Backend::X11,
    }
}

fn backend() -> Backend {
    let read = |name: &str| std::env::var(name).ok();

    backend_for(
        read("UNKVOID_CAPTURE").as_deref(),
        read("XDG_SESSION_TYPE").as_deref(),
        read("WAYLAND_DISPLAY").as_deref(),
    )
}

pub fn uses_system_picker() -> bool {
    backend() == Backend::Portal
}

/// A janela escolhida é lida pelo `xid` dela, e não a tela inteira: no X11 escolher uma janela
/// transmitia todos os monitores, com o e-mail e a conversa junto — e no XWayland do WSLg a raiz
/// sai preta, só a janela tem imagem.
fn x11_source(show_cursor: bool, region: Option<Monitor>, window: Option<u64>) -> String {
    let area = match window {
        Some(id) => format!("xid={id}"),
        None => region.map(Monitor::area).unwrap_or_default(),
    };

    format!("ximagesrc use-damage=false show-pointer={show_cursor} {area}")
}

fn window_of(source: CaptureSource) -> Option<u64> {
    match source {
        CaptureSource::Window(id) => Some(id),
        _ => None,
    }
}

/// Tela parada no PipeWire não gera buffer, e sem quadro novo o encoder não solta o
/// keyframe por segundo que quem entra na sala espera. O `keepalive-time` reenvia o
/// último quadro a cada intervalo (com o relógio de agora) e o `videorate` acerta a
/// cadência, que é o que o `ximagesrc use-damage=false` já entrega no X11. O `videorate`
/// vem antes da conversão para que o excedente de um monitor de 144 Hz caia sem custar
/// `videoconvert`.
///
/// `fd` é o do PipeWire que o portal deu, e o `path=` é o alvo explícito do stream. Dentro do
/// processo não dá para tirar o `PIPEWIRE_NODE` do ambiente só do pipeline, como se fazia com
/// o `gst-launch`; o alvo explícito é o que tem de prevalecer sobre ele (passo 9 do roteiro
/// do Wayland no `docs/ESTADO.md`).
fn portal_source(fd: i32, node: u32, frame_rate: u32) -> String {
    format!(
        "pipewiresrc fd={fd} path={node} do-timestamp=true keepalive-time={} ! videorate",
        1000 / frame_rate.max(1)
    )
}

/// Da origem ao pipe, no tamanho que o `fit` deu — par nos dois lados. A altura não pode
/// sair da proporção do que chegar: uma janela do portal com altura ímpar chegava ímpar ao
/// x264, que recusa 4:2:0 ímpar e derrubava a transmissão; e a janela que muda de tamanho
/// no meio trocava o tamanho do encoder no ar. Com os dois fixos, o que chegar maior (monitor
/// com escala) ou noutra proporção ganha tarja preta do `videoscale`, não outro tamanho.
fn screen_pipeline(source: &str, frame_rate: u32, (width, height): (u32, u32), format: &str, encoder: &str) -> String {
    format!(
        "{source} ! video/x-raw,framerate={frame_rate}/1 \
         ! videoconvert ! videoscale \
         ! video/x-raw,format={format},colorimetry=bt709,width={width},height={height},pixel-aspect-ratio=1/1 \
         ! {encoder} ! {VIDEO_SINK}"
    )
}

/// Onde o H.264 sai para o app. Cheio, segura o encoder em vez de largar quadro: quadro
/// comprimido que some estraga a imagem até o keyframe seguinte, e era o que o cano do
/// `gst-launch` fazia — encher e esperar.
const VIDEO_SINK: &str = "appsink name=sink sync=false max-buffers=8";

fn is_screen(source: CaptureSource) -> bool {
    matches!(
        source,
        CaptureSource::PrimaryDisplay | CaptureSource::Display(_) | CaptureSource::Window(_)
    )
}

/// O que o portal promete quando não diz o tamanho do stream (o campo é opcional).
const PORTAL_FALLBACK_SIZE: (u32, u32) = (1920, 1080);

/// Uma sessão do portal já com a escolha da pessoa.
///
/// A conexão D-Bus é só dela: o portal encerra a captura quando a conexão some, então o
/// app morrer, ou o `Close` não chegar, não deixa a tela sendo lida por ninguém.
struct PortalSession {
    proxy: Screencast,
    session: Arc<Session<Screencast>>,
    node: u32,
    /// ponytail: é o tamanho em coordenadas do compositor. Num monitor com escala o stream
    /// é maior, e a qualidade fica limitada à largura lógica; o tamanho real só vem das
    /// caps do PipeWire, que exigiriam a biblioteca ligada ao binário.
    size: (u32, u32),
}

impl PortalSession {
    /// Abre o seletor do sistema e espera pela pessoa, o tempo que ela levar.
    fn negotiate(show_cursor: bool) -> Result<Self, CaptureError> {
        async_io::block_on(async {
            let connection = ashpd::zbus::connection::Builder::session()?.build().await?;
            let proxy = Screencast::with_connection(connection).await?;
            let cursor = cursor_mode(proxy.available_cursor_modes().await.unwrap_or_default(), show_cursor);
            let sources = source_types(proxy.available_source_types().await.unwrap_or_default());
            let session = Arc::new(proxy.create_session(Default::default()).await?);

            let mut portal = Self { proxy, session, node: 0, size: PORTAL_FALLBACK_SIZE };

            portal
                .proxy
                .select_sources(
                    &portal.session,
                    SelectSourcesOptions::default()
                        .set_cursor_mode(cursor)
                        .set_sources(sources)
                        .set_multiple(false),
                )
                .await?
                .response()?;

            let chosen = portal.proxy.start(&portal.session, None, Default::default()).await?.response()?;

            let Some(stream) = chosen.streams().first() else {
                return Ok(None);
            };

            portal.node = stream.pipe_wire_node_id();

            match stream.size() {
                Some((width, height)) if width > 0 && height > 0 => portal.size = (width as u32, height as u32),
                _ => tracing::warn!("captura: o portal não disse o tamanho da origem, supondo 1920x1080"),
            }

            tracing::info!(node = portal.node, size = ?portal.size, "captura: origem escolhida no portal");

            Ok(Some(portal))
        })
        .map_err(portal_error)?
        .ok_or_else(|| CaptureError::Platform("o portal respondeu sem nenhuma tela".into()))
    }

    /// Um fd novo para o PipeWire. Um por `gst-launch`: o socket guarda o estado do
    /// cliente que o usou, e o filho seguinte não pode continuar a conversa do anterior.
    fn remote(&self) -> Result<OwnedFd, CaptureError> {
        async_io::block_on(self.proxy.open_pipe_wire_remote(&self.session, Default::default()))
            .map_err(portal_error)
    }
}

impl Drop for PortalSession {
    fn drop(&mut self) {
        let session = Arc::clone(&self.session);

        // Noutra thread: quem larga a sessão pode ser uma thread do tokio, e uma chamada
        // D-Bus a um portal travado a seguraria.
        std::thread::spawn(move || {
            if let Err(error) = async_io::block_on(session.close()) {
                tracing::warn!(error = %error, "captura: o portal não fechou a sessão a pedido; ela cai com a conexão");
            }
        });
    }
}

fn portal_error(error: ashpd::Error) -> CaptureError {
    match error {
        ashpd::Error::Response(ResponseError::Cancelled) => CaptureError::Cancelled,
        other => CaptureError::Platform(format!(
            "o portal de captura de tela falhou ({other}); confira o xdg-desktop-portal e o backend do seu ambiente"
        )),
    }
}

/// O cursor embutido na imagem ou fora dela, se o portal souber fazer o que foi pedido:
/// modo que ele não anuncia é recusado ("Unavailable cursor mode") e derruba a sessão.
/// Sem dizer nada o portal esconde o cursor.
fn cursor_mode(available: BitFlags<CursorMode>, show_cursor: bool) -> Option<CursorMode> {
    let wanted = if show_cursor { CursorMode::Embedded } else { CursorMode::Hidden };

    available.contains(wanted).then_some(wanted)
}

/// Monitor e janela, menos o que este portal não anuncia: o do wlroots só tem monitor, e
/// o que cada backend faz com um tipo que não conhece é problema que não precisa existir.
fn source_types(available: BitFlags<SourceType>) -> BitFlags<SourceType> {
    let wanted = SourceType::Monitor | SourceType::Window;
    let offered = wanted & available;

    if offered.is_empty() { wanted } else { offered }
}

/// A sessão que o `prepare` negociou, à espera do `start`, e a que está no ar — fraca,
/// porque quem a segura é o capturador. Trocar a qualidade refaz a captura, e é por esta
/// que ela acha a sessão aberta em vez de abrir o seletor do sistema outra vez.
struct PortalSlots {
    prepared: Option<Arc<PortalSession>>,
    active: Weak<PortalSession>,
}

static PORTAL: Mutex<PortalSlots> = Mutex::new(PortalSlots { prepared: None, active: Weak::new() });

fn portal_slots() -> MutexGuard<'static, PortalSlots> {
    PORTAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A sessão que vale agora. `consume` é o `start`: a recém-negociada sai da espera e
/// passa a ser a que está no ar.
///
/// Nunca negocia por conta própria: quem chama daqui pode estar com o cadeado da sessão
/// de envio na mão, e esperar a pessoa ali pararia voz e câmera junto.
fn portal_session(consume: bool) -> Result<Arc<PortalSession>, CaptureError> {
    let mut slots = portal_slots();
    let prepared = if consume { slots.prepared.take() } else { slots.prepared.clone() };

    let session = prepared.or_else(|| slots.active.upgrade()).ok_or_else(|| {
        CaptureError::Platform("no Wayland a tela é escolhida no `prepare`, antes de ligar a captura".into())
    })?;

    if consume {
        slots.active = Arc::downgrade(&session);
    }

    Ok(session)
}

pub(crate) fn prepare(config: &CaptureConfig) -> Result<(), CaptureError> {
    if ! is_screen(config.source) || backend() != Backend::Portal {
        return Ok(());
    }

    let show_cursor = config.show_cursor;

    let session = std::thread::spawn(move || PortalSession::negotiate(show_cursor))
        .join()
        .map_err(|_| CaptureError::Platform("o portal de captura de tela respondeu o que não devia".into()))??;

    portal_slots().prepared = Some(Arc::new(session));

    Ok(())
}

pub(crate) fn discard_prepared() {
    portal_slots().prepared = None;
}

/// A câmera sobe pequena: é um cartão ao lado da tela, não a tela.
const CAMERA_SIZE: (u32, u32) = (640, 360);

/// O fim de todo pipeline de áudio: 48 kHz estéreo em `f32`, como o `AudioEncoder` quer.
const AUDIO_TAIL: &str = "audioconvert ! audioresample \
    ! audio/x-raw,format=F32LE,rate=48000,channels=2,layout=interleaved ! fdsink fd=1 sync=false";

/// O microfone padrão, limpo pelo `webrtcdsp` quando a distro o tem (plugins bad).
///
/// O cancelamento de eco fica desligado: ele exige um `webrtcechoprobe` no mesmo pipeline,
/// e o som dos outros toca em outro processo (`watch.rs`). Ligado sem a sonda, o
/// `webrtcdsp` se recusa a iniciar e derruba o microfone inteiro — era o que acontecia.
///
/// ponytail: sem eco cancelado, quem usa caixa de som em vez de fone devolve a voz dos
/// outros. Teto: tocar o som da sala e capturar o microfone no mesmo pipeline, com a sonda.
fn microphone_pipeline() -> String {
    let cleanup = if has_webrtcdsp() {
        "! audioconvert ! audio/x-raw,format=S16LE,rate=48000,channels=2,layout=interleaved \
         ! webrtcdsp echo-cancel=false noise-suppression=true gain-control=true "
    } else {
        ""
    };

    format!("pulsesrc device=@DEFAULT_SOURCE@ {cleanup}! {AUDIO_TAIL}")
}

/// A taxa da câmera, em kbit/s.
const CAMERA_BITRATE: u32 = 800;

/// `UNKVOID_CAMERA_SOURCE` troca a webcam por qualquer origem do GStreamer (por exemplo
/// `videotestsrc is-live=true pattern=ball`): é como se prova a câmera de ponta a ponta numa
/// máquina sem webcam, e num contêiner, que não tem `/dev/video*`.
fn camera_source(index: u32) -> String {
    std::env::var("UNKVOID_CAMERA_SOURCE")
        .ok()
        .filter(|source| !source.trim().is_empty())
        .unwrap_or_else(|| format!("v4l2src device=/dev/video{index}"))
}

/// O encoder da tela, mas em 640x360 a 30 fps e 800 kbit/s: um cartão pequeno não
/// precisa de mais, e é banda que a tela de alguém está usando. O pixel quadrado é o que
/// põe tarja na webcam 4:3 em vez de achatá-la: sem ele o `videoscale` esticava a imagem em
/// pixel retangular, e o decodificador do Windows, que ignora a proporção do pixel, mostrava
/// o rosto largo.
fn camera_pipeline(index: u32) -> String {
    let (width, height) = CAMERA_SIZE;
    let (format, encoder) = encoder_tail(LinuxCapturer::video_encoder(), 30, CAMERA_BITRATE);

    format!(
        "{} ! videoconvert ! videoscale ! videorate \
         ! video/x-raw,format={format},colorimetry=bt709,width={width},height={height},framerate=30/1,pixel-aspect-ratio=1/1 \
         ! {encoder} ! {VIDEO_SINK}",
        camera_source(index)
    )
}

/// Os encoders de H.264 da placa, na ordem em que são tentados: NVIDIA, VA-API novo (Intel
/// e AMD, integrada inclusive) e o VA-API antigo das distros que ainda não têm o novo.
const HARDWARE_H264_ENCODERS: [&str; 3] = ["nvh264enc", "vah264enc", "vaapih264enc"];

/// Quanto a sondagem espera por um encoder. Um elemento que não existe falha em
/// milissegundos; um driver que pendura na abertura não pode segurar a transmissão.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Quanto uma pergunta ao X espera. Todo comando do `text` — `xrandr`, `xdpyinfo` — abre
/// uma conexao com o servidor grafico, e um X que nao responde nao devolve erro: ele
/// pendura. Sem isto o `xrandr` de uma sessao sem X levou cinco minutos para desistir, e
/// nesse tempo quem clicou em compartilhar so ve o app travado.
const X_QUERY_TIMEOUT: &str = "3";

/// O formato cru que o encoder quer e o pipeline dele até o H.264 pronto, sem a saída. O
/// encoder se chama `encoder`: é por esse nome que o `EncoderControl` o acha.
///
/// Todos terminam iguais: H.264 byte-stream em constrained baseline (o `profile-level-id`
/// que o servidor anuncia), um quadro por buffer e SPS/PPS na frente de cada IDR — é disso
/// que quem empacota e quem entra no meio dependem. Quem garante as duas
/// últimas é o `h264parse`: ele insere o AUD que falta e repete SPS/PPS por IDR
/// (`config-interval=-1`), o que o x264 fazia sozinho e os de placa nem sempre fazem.
///
/// ponytail: a conversão para NV12/I420 continua no `videoconvert`, na CPU, para todos;
/// `cudaconvert`/`vapostproc` a levariam para a placa quando o custo aparecer.
fn encoder_tail(element: &str, key_interval: u32, bitrate: u32) -> (&'static str, String) {
    let (format, encoder) = match element {
        "nvh264enc" => (
            "NV12",
            format!("nvh264enc name=encoder preset=low-latency-hp zerolatency=true bframes=0 gop-size={key_interval} bitrate={bitrate}"),
        ),
        "vah264enc" => (
            "NV12",
            format!("vah264enc name=encoder rate-control=cbr b-frames=0 cabac=false dct8x8=false key-int-max={key_interval} bitrate={bitrate}"),
        ),
        "vaapih264enc" => (
            "NV12",
            format!("vaapih264enc name=encoder rate-control=cbr max-bframes=0 keyframe-period={key_interval} bitrate={bitrate}"),
        ),
        // `vbv-buf-capacity=100` (ms) é o que segura o pico de um keyframe dentro de um
        // décimo de segundo de banda, em vez de um segundo inteiro.
        _ => (
            "I420",
            format!(
                "x264enc name=encoder tune=zerolatency speed-preset=ultrafast byte-stream=true aud=true \
                 key-int-max={key_interval} bitrate={bitrate} vbv-buf-capacity=100 threads=0"
            ),
        ),
    };

    (
        format,
        format!(
            "{encoder} ! h264parse config-interval=-1 \
             ! video/x-h264,stream-format=byte-stream,alignment=au,profile=constrained-baseline"
        ),
    )
}

/// Se o encoder abre nesta máquina: um quadro de teste pelo mesmo trecho da transmissão.
fn encoder_opens(element: &str) -> bool {
    let (format, encoder) = encoder_tail(element, 30, 1_000);
    let pipeline = format!(
        "videotestsrc num-buffers=1 ! videoconvert ! video/x-raw,format={format},width=640,height=360 ! {encoder} ! fakesink"
    );

    let Ok(mut child) = Command::new("gst-launch-1.0")
        .arg("-q")
        .args(pipeline.split_whitespace())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };

    let started = Instant::now();

    while started.elapsed() < PROBE_TIMEOUT {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }

    tracing::warn!(encoder = element, "captura: a sondagem do encoder não terminou a tempo");

    let _ = child.kill();
    let _ = child.wait();

    false
}

/// Se o `webrtcdsp` existe. Perguntado uma vez por processo: o `gst-inspect` leva
/// dezenas de milissegundos, e ligar o microfone acontecia com a sessão trancada.
fn has_webrtcdsp() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();

    *FOUND.get_or_init(|| {
        Command::new("gst-inspect-1.0")
            .arg("webrtcdsp")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Ordem numérica e um nó por nome, o de menor índice.
fn dedupe_cameras(found: impl Iterator<Item = (u32, String)>) -> Vec<(String, String)> {
    let mut found: Vec<(u32, String)> = found.collect();

    found.sort();
    found.dedup_by(|later, earlier| later.1 == earlier.1);

    found
        .into_iter()
        .map(|(index, name)| (format!("/dev/video{index}"), name))
        .collect()
}

/// Guarda a última linha de erro do gst: é o que aparece no app quando a captura não
/// gera quadro nenhum — sem isto o diagnóstico culpava a rede.
fn watch_stderr(child: &mut Child, error: Arc<Mutex<Option<String>>>) {
    let Some(stderr) = child.stderr.take() else {
        return;
    };

    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            tracing::warn!(line = %line, "gst");

            if (line.starts_with("ERROR") || line.contains("rror"))
                && let Ok(mut slot) = error.lock()
            {
                *slot = Some(line);
            }
        }
    });
}

/// O vídeo dentro do processo, da origem ao `appsink`.
struct VideoPipeline {
    pipeline: gst::Pipeline,
    /// O fd do PipeWire que o portal deu, vivo enquanto o `pipewiresrc` o usa.
    _remote: Option<OwnedFd>,
}

impl VideoPipeline {
    /// Sobe o pipeline e a thread que entrega cada quadro ao callback — fora da thread do
    /// GStreamer, para empacotar e mandar não atrasar a captura. O erro do GStreamer vai
    /// para `error`: é o que o app mostra quando a captura não gera quadro nenhum.
    fn start(
        description: &str,
        (remote, (width, height), bitrate): (Option<OwnedFd>, (u32, u32), u32),
        (frames, error): (Arc<AtomicU64>, Arc<Mutex<Option<String>>>),
        on_event: Arc<dyn Fn(CaptureEvent) + Send + Sync>,
    ) -> Result<Self, CaptureError> {
        let refused = |what: &str, failure: &dyn std::fmt::Display| {
            CaptureError::Platform(format!("{what} ({failure}); instale os plugins good, bad e ugly do GStreamer"))
        };

        gst::init().map_err(|failure| refused("o GStreamer não iniciou", &failure))?;

        let pipeline = gst::parse::launch(description)
            .map_err(|failure| refused("o pipeline da captura não montou", &failure))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| CaptureError::Platform("o pipeline da captura não é um pipeline".into()))?;
        let video = Self { pipeline, _remote: remote };
        let (Some(encoder), Some(sink)) = (video.pipeline.by_name("encoder"), video.pipeline.by_name("sink")) else {
            return Err(CaptureError::Platform("o pipeline da captura veio sem encoder ou sem saída".into()));
        };
        let appsink = sink
            .clone()
            .downcast::<gst_app::AppSink>()
            .map_err(|_| CaptureError::Platform("a saída da captura não é um appsink".into()))?;
        let control = EncoderControl { encoder, sink, launched: bitrate };

        if let Some(bus) = video.pipeline.bus() {
            bus.set_sync_handler(move |_, message| {
                if let gst::MessageView::Error(failure) = message.view() {
                    let line = format!("{} ({})", failure.error(), failure.debug().map(|debug| debug.to_string()).unwrap_or_default());

                    tracing::warn!(%line, "gst");

                    if let Ok(mut slot) = error.lock() {
                        *slot = Some(line);
                    }
                }

                gst::BusSyncReply::Drop
            });
        }

        video
            .pipeline
            .set_state(gst::State::Playing)
            .map_err(|failure| refused("a captura não começou", &failure))?;

        let clocked = video.pipeline.downgrade();

        std::thread::Builder::new()
            .name("unkvoid-captura".into())
            .spawn(move || {
                // Com o pipeline parado o `appsink` esvazia, e o `pull_sample` devolve erro.
                while let Ok(sample) = appsink.pull_sample() {
                    let timestamp_ns = captured_at(clocked.upgrade().as_ref(), &sample);
                    let Some(buffer) = sample.buffer() else {
                        continue;
                    };
                    let Ok(map) = buffer.map_readable() else {
                        continue;
                    };
                    let data = without_delimiter(&map).to_vec();

                    frames.fetch_add(1, Ordering::Relaxed);

                    on_event(CaptureEvent::Video(VideoFrame {
                        width,
                        height,
                        timestamp_ns,
                        surface: Some(EncodedVideo { keyframe: has_idr(&data), data, encoder: Some(control.clone()) }),
                    }));
                }

                tracing::info!("captura: o vídeo parou");
            })
            .map_err(|failure| refused("a thread da captura não abriu", &failure))?;

        Ok(video)
    }
}

impl Drop for VideoPipeline {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// O zero do relógio dos quadros, um só para o processo inteiro. Trocar a qualidade, refazer
/// a captura travada ou descer um degrau abre outro pipeline no mesmo producer; com o relógio
/// recomeçando do zero em cada um, o RTP de quem assiste andava um quadro só no lugar do tempo
/// que a troca levou, e a imagem ficava esse tanto atrás do som até a espera do `Playout` descer.
fn capture_epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();

    *EPOCH.get_or_init(Instant::now)
}

/// Mais que isto entre a captura e a saída do encoder não é atraso, é relógio que não se
/// entende com o do buffer: vale a hora da saída.
const MOST_ENCODER_DELAY: Duration = Duration::from_secs(1);

/// Quando o quadro foi capturado, em nanossegundos desde o `capture_epoch`: a hora em que saiu
/// do encoder menos o quanto ele passou no pipeline, medido no relógio do próprio pipeline. É a
/// hora da captura, e não a da saída, que o RTP carrega: o x264 leva de 2 a 15 ms por quadro, e
/// a hora da saída tremia junto. O carimbo passa pelo segmento porque o encoder o desloca (o
/// `x264enc` soma mil horas a todo carimbo que sai).
fn captured_at(pipeline: Option<&gst::Pipeline>, sample: &gst::Sample) -> u64 {
    let now = Instant::now();
    let in_pipeline = pipeline
        .and_then(|pipeline| {
            let pts = sample.buffer()?.pts()?;
            let running = sample.segment()?.downcast_ref::<gst::ClockTime>()?.to_running_time(pts)?;
            let captured = pipeline.base_time()? + running;

            Some(Duration::from_nanos(pipeline.clock()?.time().nseconds().checked_sub(captured.nseconds())?))
        })
        .filter(|delay| *delay < MOST_ENCODER_DELAY)
        .unwrap_or_default();

    now.checked_sub(in_pipeline).unwrap_or(now).saturating_duration_since(capture_epoch()).as_nanos() as u64
}

/// O áudio do pipe, em blocos de 20 ms, para o callback.
fn read_audio(
    child: &mut Child,
    chunks: Arc<AtomicU64>,
    on_event: Arc<dyn Fn(CaptureEvent) + Send + Sync>,
) {
    let mut stdout = child.stdout.take().expect("stdout piped");

    std::thread::spawn(move || {
        let mut block = vec![0_u8; AUDIO_BLOCK_BYTES];

        while stdout.read_exact(&mut block).is_ok() {
            chunks.fetch_add(1, Ordering::Relaxed);

            on_event(CaptureEvent::Audio(AudioChunk {
                sample_rate: 48_000,
                channels: 2,
                samples: block.as_chunks::<4>().0.iter().map(|bytes| f32::from_le_bytes(*bytes)).collect(),
            }));
        }

        tracing::warn!("captura: o gst-launch de áudio terminou (sem PulseAudio/PipeWire?)");
    });
}

fn launch(pipeline: &str, stdin: Stdio) -> Result<Child, CaptureError> {
    Command::new("gst-launch-1.0")
        .arg("-q")
        .args(pipeline.split_whitespace())
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            CaptureError::Platform(format!(
                "gst-launch-1.0 não abriu ({error}); instale gstreamer1.0-tools e os plugins good/ugly"
            ))
        })
}

/// Um monitor como o `xrandr` o descreve: tamanho e posição dentro da tela do X.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Monitor {
    width: u32,
    height: u32,
    x: u32,
    y: u32,
}

impl Monitor {
    /// O recorte para o `ximagesrc`. As bordas são inclusivas.
    fn area(self) -> String {
        format!(
            "startx={} starty={} endx={} endy={}",
            self.x,
            self.y,
            self.x + self.width - 1,
            self.y + self.height - 1
        )
    }
}

/// Os monitores ligados, o principal primeiro. Dois monitores são UMA tela para o X;
/// sem isto a captura mandava os dois lado a lado, espremidos em 16:9.
fn monitors() -> Vec<Monitor> {
    let Some(output) = text("xrandr", &["--current"]) else {
        return Vec::new();
    };

    parse_monitors(&output)
}

fn parse_monitors(xrandr: &str) -> Vec<Monitor> {
    let mut found: Vec<(bool, Monitor)> = xrandr
        .lines()
        .filter(|line| line.contains(" connected "))
        .filter_map(|line| {
            let primary = line.contains(" primary ");
            let geometry = line.split_whitespace().find(|word| {
                word.contains('x') && word.matches('+').count() == 2
            })?;
            let (size, offset) = geometry.split_once('+')?;
            let (width, height) = size.split_once('x')?;
            let (x, y) = offset.split_once('+')?;

            Some((
                primary,
                Monitor {
                    width: width.parse().ok()?,
                    height: height.parse().ok()?,
                    x: x.parse().ok()?,
                    y: y.parse().ok()?,
                },
            ))
        })
        .collect();

    found.sort_by_key(|(primary, _)| ! primary);

    found.into_iter().map(|(_, monitor)| monitor).collect()
}

/// O monitor que uma escolha do seletor quer dizer. `None` é a tela do X inteira.
fn region(source: CaptureSource) -> Option<Monitor> {
    let monitors = monitors();

    match source {
        CaptureSource::Display(id) => monitors.get(id.checked_sub(1)? as usize).copied(),
        CaptureSource::PrimaryDisplay => monitors.first().copied(),
        CaptureSource::Window(_) | CaptureSource::Camera(_) | CaptureSource::Microphone => None,
    }
}

fn text(program: &str, args: &[&str]) -> Option<String> {
    Command::new("timeout")
        .arg(X_QUERY_TIMEOUT)
        .arg(program)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Tamanho da tela pelo X, para o seletor mostrar. Sem `xdpyinfo` nem `xrandr` fica
/// sem número — a captura em si não depende disto.
fn screen_size() -> Option<(u32, u32)> {
    let pair = |numbers: &str| {
        let (width, height) = numbers.trim().split_once('x')?;

        Some((width.trim().parse().ok()?, height.trim().parse().ok()?))
    };

    if let Some(output) = text("xdpyinfo", &[])
        && let Some(line) = output.lines().find(|line| line.trim_start().starts_with("dimensions:"))
        && let Some(size) = line.split_whitespace().nth(1).and_then(pair)
    {
        return Some(size);
    }

    let output = text("xrandr", &["--current"])?;
    let line = output.lines().find(|line| line.contains("current"))?;
    let after = line.split("current").nth(1)?;
    let numbers = after.split(',').next()?.replace(' ', "");

    pair(&numbers)
}

const AUD: [u8; 5] = [0, 0, 0, 1, 9];

/// O quadro sem o delimitador (`AUD`) da frente, quando há um. Cada buffer do `appsink` já
/// é um quadro inteiro (`alignment=au`); o delimitador não vai para a rede.
fn without_delimiter(data: &[u8]) -> &[u8] {
    let Some(rest) = data.strip_prefix(&AUD) else {
        return data;
    };

    match find(rest, &[0, 0, 1], 0) {
        // O código de início pode ter quatro bytes; o zero a mais fica com o quadro.
        Some(start) if start > 0 && rest[start - 1] == 0 => &rest[start - 1..],
        Some(start) => &rest[start..],
        None => &[],
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| position + from)
}

fn has_idr(data: &[u8]) -> bool {
    data.windows(4)
        .any(|window| window[..3] == [0, 0, 1] && window[3] & 0x1f == 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_monitors_become_two_displays_primary_first() {
        let xrandr = "Screen 0: minimum 320 x 200, current 3840 x 1080, maximum 16384 x 16384\n\
            DP-1 connected 1920x1080+1920+0 (normal left inverted right x axis y axis) 527mm x 296mm\n\
            HDMI-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 527mm x 296mm\n\
            DP-2 disconnected (normal left inverted right x axis y axis)\n\
               1920x1080     60.00*+\n";

        let monitors = parse_monitors(xrandr);

        assert_eq!(monitors.len(), 2);
        assert_eq!(monitors[0], Monitor { width: 1920, height: 1080, x: 0, y: 0 });
        assert_eq!(monitors[1].x, 1920);
        assert_eq!(monitors[1].area(), "startx=1920 starty=0 endx=3839 endy=1079");
    }

    #[test]
    fn wayland_goes_through_the_portal_and_the_override_wins() {
        assert_eq!(backend_for(None, Some("x11"), None), Backend::X11);
        assert_eq!(backend_for(None, None, None), Backend::X11, "sem sessão gráfica declarada segue como sempre foi");
        assert_eq!(backend_for(None, Some("wayland"), None), Backend::Portal);
        assert_eq!(backend_for(None, Some("tty"), Some("wayland-0")), Backend::Portal, "compositor aberto à mão de um tty");
        assert_eq!(backend_for(None, Some("x11"), Some("")), Backend::X11, "variável vazia não é Wayland");

        assert_eq!(backend_for(Some("x11"), Some("wayland"), Some("wayland-0")), Backend::X11);
        assert_eq!(backend_for(Some("portal"), Some("x11"), None), Backend::Portal);
        assert_eq!(backend_for(Some("pipewire"), Some("x11"), None), Backend::X11, "valor desconhecido não força nada");
        assert_eq!(backend_for(Some(""), Some("wayland"), None), Backend::Portal);
    }

    #[test]
    fn the_two_screen_pipelines_differ_only_in_the_source() {
        let (format, encoder) = encoder_tail("x264enc", 60, 10_000);
        let region = Monitor { width: 1920, height: 1080, x: 1920, y: 0 };
        let words = |pipeline: String| pipeline.split_whitespace().map(str::to_string).collect::<Vec<_>>().join(" ");

        let x11 = words(screen_pipeline(&x11_source(false, Some(region), None), 60, (1920, 1080), format, &encoder));
        let portal = words(screen_pipeline(&portal_source(12, 47, 60), 60, (1920, 1080), format, &encoder));
        let shared = words(format!(
            "! video/x-raw,framerate=60/1 ! videoconvert ! videoscale \
             ! video/x-raw,format=I420,colorimetry=bt709,width=1920,height=1080,pixel-aspect-ratio=1/1 ! {encoder} ! {VIDEO_SINK}"
        ));

        assert_eq!(
            x11,
            format!("ximagesrc use-damage=false show-pointer=false startx=1920 starty=0 endx=3839 endy=1079 {shared}")
        );
        assert_eq!(
            portal,
            format!("pipewiresrc fd=12 path=47 do-timestamp=true keepalive-time=16 ! videorate {shared}"),
            "o fd é o que o portal deu, e tela parada continua gerando quadro"
        );

        assert_eq!(words(x11_source(true, None, None)), "ximagesrc use-damage=false show-pointer=true");
        assert_eq!(
            words(x11_source(false, Some(region), window_of(CaptureSource::Window(6_291_458)))),
            "ximagesrc use-damage=false show-pointer=false xid=6291458",
            "a janela escolhida, e não o monitor inteiro"
        );
        assert!(portal_source(5, 3, 30).contains("keepalive-time=33 "), "um reenvio por quadro a 30 fps");
        assert!(portal_source(5, 3, 0).contains("keepalive-time=1000 "), "fps zero não divide por zero");
    }

    #[test]
    fn the_portal_is_only_asked_for_what_it_offers() {
        let every_cursor = CursorMode::Hidden | CursorMode::Embedded | CursorMode::Metadata;

        assert_eq!(cursor_mode(every_cursor, true), Some(CursorMode::Embedded));
        assert_eq!(cursor_mode(every_cursor, false), Some(CursorMode::Hidden));
        assert_eq!(cursor_mode(CursorMode::Hidden.into(), true), None, "sem cursor embutido, vale o padrão do portal");
        assert_eq!(cursor_mode(BitFlags::empty(), false), None, "portal antigo não diz o que sabe");

        assert_eq!(source_types(SourceType::Monitor.into()), BitFlags::from(SourceType::Monitor), "wlroots");
        assert_eq!(
            source_types(SourceType::Monitor | SourceType::Window | SourceType::Virtual),
            SourceType::Monitor | SourceType::Window
        );
        assert_eq!(source_types(BitFlags::empty()), SourceType::Monitor | SourceType::Window);
    }

    #[test]
    fn only_a_screen_source_waits_for_the_system_picker() {
        assert!(is_screen(CaptureSource::PrimaryDisplay));
        assert!(is_screen(CaptureSource::Display(2)));
        assert!(is_screen(CaptureSource::Window(7)));
        assert!(! is_screen(CaptureSource::Camera(0)));
        assert!(! is_screen(CaptureSource::Microphone));
    }

    #[test]
    fn a_cancelled_picker_is_not_a_platform_failure() {
        assert!(matches!(
            portal_error(ashpd::Error::Response(ResponseError::Cancelled)),
            CaptureError::Cancelled
        ));
        assert!(matches!(
            portal_error(ashpd::Error::Response(ResponseError::Other)),
            CaptureError::Platform(_)
        ));
        assert!(matches!(portal_error(ashpd::Error::NoResponse), CaptureError::Platform(_)));
    }

    #[test]
    fn every_encoder_ends_in_the_same_byte_stream() {
        for element in HARDWARE_H264_ENCODERS.into_iter().chain(["x264enc"]) {
            let (format, tail) = encoder_tail(element, 60, 5_000);

            assert!(tail.starts_with(&format!("{element} name=encoder ")), "o `EncoderControl` acha o encoder pelo nome: {tail}");
            assert!(["NV12", "I420"].contains(&format), "{element}: {format}");
            assert!(tail.contains("=60 ") && tail.contains("bitrate=5000"), "{element}: keyframe por segundo e a taxa: {tail}");
            assert!(
                tail.split_whitespace().collect::<Vec<_>>().join(" ").ends_with(
                    "! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream,alignment=au,profile=constrained-baseline"
                ),
                "{element}: sem o h264parse o pipe perde o AUD ou o SPS/PPS por IDR: {tail}"
            );
        }
    }

    #[test]
    fn cameras_come_in_numeric_order_and_the_metadata_node_is_dropped() {
        let found = [
            (10, "Webcam B".to_string()),
            (2, "Webcam A".to_string()),
            (3, "Webcam A".to_string()),
            (11, "Webcam B".to_string()),
        ];

        assert_eq!(
            dedupe_cameras(found.into_iter()),
            [
                ("/dev/video2".to_string(), "Webcam A".to_string()),
                ("/dev/video10".to_string(), "Webcam B".to_string()),
            ]
        );
    }

    type Seen = Arc<Mutex<Vec<(Instant, EncodedVideo)>>>;

    /// O pipeline da captura com o x264, sem tela: o `videotestsrc` no lugar da origem.
    fn running(pattern: &str, key_interval: u32, bitrate: u32) -> (VideoPipeline, Seen) {
        let (format, encoder) = encoder_tail("x264enc", key_interval, bitrate);
        let description = format!(
            "videotestsrc is-live=true pattern={pattern} ! video/x-raw,width=640,height=360,framerate=30/1 \
             ! videoconvert ! video/x-raw,format={format} ! {encoder} ! {VIDEO_SINK}"
        );
        let seen = Seen::default();
        let on_event = {
            let seen = Arc::clone(&seen);

            move |event| {
                if let CaptureEvent::Video(frame) = event
                    && let Some(surface) = frame.surface
                {
                    seen.lock().expect("a lista").push((Instant::now(), surface));
                }
            }
        };
        let video = VideoPipeline::start(&description, (None, (640, 360), bitrate * 1000), (Arc::default(), Arc::default()), Arc::new(on_event))
            .expect("o pipeline subiu");

        (video, seen)
    }

    fn control(seen: &Seen) -> EncoderControl {
        seen.lock().expect("a lista").last().and_then(|(_, frame)| frame.encoder.clone()).expect("o quadro traz o encoder")
    }

    /// O PLI de quem assiste vira keyframe na hora, e não só no fim do GOP. Contra o GStreamer
    /// de verdade: `cargo test -p capture -- --ignored`.
    #[test]
    #[ignore]
    fn a_keyframe_comes_when_asked_and_not_only_once_per_gop() {
        let (_video, seen) = running("ball", 300, 1_000);

        std::thread::sleep(Duration::from_millis(1_500));

        let opening = seen.lock().expect("a lista").iter().filter(|(_, frame)| frame.keyframe).count();
        let asked = Instant::now();

        control(&seen).request_keyframe();
        std::thread::sleep(Duration::from_millis(700));

        let answered = seen.lock().expect("a lista").iter().any(|(at, frame)| *at > asked && frame.keyframe);

        assert_eq!(opening, 1, "num GOP de 10 s só o keyframe da abertura");
        assert!(answered, "o pedido não virou keyframe");
    }

    /// A perda baixa a taxa com a captura no ar. Contra o GStreamer de verdade.
    #[test]
    #[ignore]
    fn the_bitrate_drops_with_the_pipeline_playing() {
        let (_video, seen) = running("snow", 30, 2_000);
        let bytes_in_the_last_second = |seen: &Seen| {
            let since = Instant::now() - Duration::from_secs(1);

            seen.lock().expect("a lista").iter().filter(|(at, _)| *at > since).map(|(_, frame)| frame.data.len()).sum::<usize>()
        };

        std::thread::sleep(Duration::from_secs(2));

        let before = bytes_in_the_last_second(&seen);

        assert!(control(&seen).set_bitrate(200_000), "o x264enc aceita a taxa nova tocando");
        std::thread::sleep(Duration::from_millis(2_500));

        let after = bytes_in_the_last_second(&seen);

        assert!(after * 3 < before, "a taxa não caiu: {before} bytes/s antes, {after} depois");
    }

    /// Trocar a qualidade abre outro pipeline, e o relógio dos quadros continua de onde estava:
    /// antes ele recomeçava do zero, e o quadro novo parecia mais velho que o último do pipeline
    /// anterior. Contra o GStreamer de verdade.
    #[test]
    #[ignore]
    fn the_frame_clock_keeps_going_across_pipelines() {
        let stamps = |pattern: &str| {
            let (format, encoder) = encoder_tail("x264enc", 30, 1_000);
            let description = format!(
                "videotestsrc is-live=true pattern={pattern} ! video/x-raw,width=320,height=180,framerate=30/1 \
                 ! videoconvert ! video/x-raw,format={format} ! {encoder} ! {VIDEO_SINK}"
            );
            let seen = Arc::new(Mutex::new(Vec::new()));
            let on_event = {
                let seen = Arc::clone(&seen);

                move |event| {
                    if let CaptureEvent::Video(frame) = event {
                        seen.lock().expect("a lista").push(frame.timestamp_ns);
                    }
                }
            };
            let video = VideoPipeline::start(&description, (None, (320, 180), 1_000_000), (Arc::default(), Arc::default()), Arc::new(on_event))
                .expect("o pipeline subiu");

            std::thread::sleep(Duration::from_millis(600));
            drop(video);

            seen.lock().expect("a lista").clone()
        };

        let first = stamps("ball");

        std::thread::sleep(Duration::from_millis(400));

        let second = stamps("snow");
        let (Some(&last), Some(&next)) = (first.last(), second.first()) else {
            panic!("um dos pipelines não deu quadro: {first:?} {second:?}");
        };

        assert!(first.windows(2).all(|pair| pair[1] > pair[0]), "o relógio andou para trás dentro do pipeline: {first:?}");
        assert!(next >= last + 400_000_000, "o pipeline novo começou em {next} ns, antes do fim do anterior ({last} ns) mais a pausa");
    }

    /// A webcam 4:3 sai com tarja, em pixel quadrado, no tamanho do cartão.
    #[test]
    fn the_camera_keeps_square_pixels_at_the_card_size() {
        let words = camera_pipeline(2).split_whitespace().collect::<Vec<_>>().join(" ");

        assert!(words.starts_with("v4l2src device=/dev/video2 ! "), "{words}");
        assert!(words.contains(",width=640,height=360,framerate=30/1,pixel-aspect-ratio=1/1 "), "{words}");
    }

    #[test]
    fn the_delimiter_in_front_of_a_frame_is_dropped() {
        let keyframe = [0, 0, 0, 1, 9, 0x10, 0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x65, 3, 4];
        let delta = [0, 0, 0, 1, 9, 0x30, 0, 0, 1, 0x41, 5];

        assert_eq!(without_delimiter(&keyframe), [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x65, 3, 4]);
        assert!(has_idr(without_delimiter(&keyframe)));
        assert_eq!(without_delimiter(&delta), [0, 0, 1, 0x41, 5]);
        assert!(! has_idr(without_delimiter(&delta)));
        assert_eq!(without_delimiter(&[0, 0, 1, 0x41, 5]), [0, 0, 1, 0x41, 5], "sem delimitador, o quadro passa inteiro");
    }
}
