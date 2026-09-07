use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rtc::interceptor::Registry;
use rtc::media::Sample;
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::RTCConfigurationBuilder;
use rtc::peer_connection::configuration::interceptor_registry::register_default_interceptors;
use rtc::peer_connection::configuration::media_engine::{
    MIME_TYPE_H264, MIME_TYPE_OPUS, MediaEngine,
};
use rtc::peer_connection::sdp::RTCSessionDescription;
use rtc::peer_connection::transport::RTCIceServer;
use rtc::rtp_transceiver::PayloadType;
use rtc::rtp_transceiver::rtp_sender::{
    RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
    RtpCodecKind,
};
use tokio::sync::mpsc;
use webrtc::media_stream::track_local::TrackLocal;
use webrtc::media_stream::track_local::static_sample::TrackLocalStaticSample;
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCPeerConnectionIceEvent,
};
use webrtc::rtp_transceiver::RtpSender;
use webrtc::runtime::TokioRuntime;

use crate::EncodedFrame;

const SSRC_VIDEO: u32 = 0x1234_5678;
const SSRC_AUDIO: u32 = 0x1234_5679;

/// O que precisa chegar ao outro lado pela sinalização do SFU.
#[derive(Debug)]
pub enum Signal {
    Candidate(String),
}

#[derive(Clone)]
struct IceHandler {
    saida: mpsc::Sender<Signal>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for IceHandler {
    async fn on_ice_candidate(&self, evento: RTCPeerConnectionIceEvent) {
        if let Ok(texto) = serde_json::to_string(&evento.candidate) {
            let _ = self.saida.send(Signal::Candidate(texto)).await;
        }
    }
}

/// Conexão direta com outro participante.
///
/// P2P pesa a favor aqui: o servidor está nos EUA e as pessoas no Brasil — 139 ms de
/// ida e volta por ele, contra ~20 ms indo direto. Acima de 3 espectadores o upload
/// de quem transmite multiplica e o SFU volta a compensar.
pub struct PeerLink {
    connection: Arc<dyn PeerConnection>,
    screen: Arc<TrackLocalStaticSample>,
    audio: Arc<TrackLocalStaticSample>,
    sender: Arc<dyn RtpSender>,
    audio_sender: Arc<dyn RtpSender>,
    payload_type: PayloadType,
    audio_payload_type: PayloadType,
    frame_rate: f64,
}

impl PeerLink {
    pub async fn connect(
        ice_servers: Vec<String>,
        frame_rate: f64,
    ) -> Result<(Self, mpsc::Receiver<Signal>)> {
        let (emissor, receptor) = mpsc::channel(64);

        // H.264 porque é o que o encoder por hardware entrega. Outro codec obrigaria
        // a codificar por software, e aí a CPU volta a pesar.
        let codec = RTCRtpCodec {
            mime_type: MIME_TYPE_H264.to_owned(),
            clock_rate: 90_000,
            channels: 0,
            sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                .to_owned(),
            ..Default::default()
        };

        // Áudio do sistema em Opus. A captura já entrega sem o som do nosso próprio
        // app, então não há risco de devolver a voz de quem está na chamada.
        let codec_audio = RTCRtpCodec {
            mime_type: MIME_TYPE_OPUS.to_owned(),
            clock_rate: crate::audio::SAMPLE_RATE,
            channels: crate::audio::CHANNELS,
            sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
            ..Default::default()
        };

        // O transceiver só aceita o que o media engine conhece: sem registrar aqui,
        // adicionar a trilha falha com "unsupported codec type".
        let mut media_engine = MediaEngine::default();

        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    rtp_codec: codec.clone(),
                    payload_type: 102,
                },
                RtpCodecKind::Video,
            )
            .context("registrar H.264")?;

        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    rtp_codec: codec_audio.clone(),
                    payload_type: 111,
                },
                RtpCodecKind::Audio,
            )
            .context("registrar Opus")?;

        let registry = register_default_interceptors(Registry::new(), &mut media_engine)
            .context("interceptors")?;

        let config = RTCConfigurationBuilder::new()
            .with_ice_servers(vec![RTCIceServer {
                urls: ice_servers,
                ..Default::default()
            }])
            .build();

        let connection = PeerConnectionBuilder::new()
            .with_configuration(config)
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_runtime(Arc::new(TokioRuntime))
            .with_handler(Arc::new(IceHandler { saida: emissor }))
            .with_udp_addrs(vec!["0.0.0.0:0"])
            .build()
            .await
            .context("criar peer connection")?;

        let connection: Arc<dyn PeerConnection> = Arc::new(connection);

        let screen = Arc::new(
            TrackLocalStaticSample::new(trilha(
                "screen",
                "tela",
                RtpCodecKind::Video,
                SSRC_VIDEO,
                codec,
            ))
            .context("montar a trilha da tela")?,
        );

        let audio = Arc::new(
            TrackLocalStaticSample::new(trilha(
                "audio",
                "som",
                RtpCodecKind::Audio,
                SSRC_AUDIO,
                codec_audio,
            ))
            .context("montar a trilha de áudio")?,
        );

        let sender = connection
            .add_track(Arc::clone(&screen) as Arc<dyn TrackLocal>)
            .await
            .context("adicionar a trilha de vídeo")?;

        let audio_sender = connection
            .add_track(Arc::clone(&audio) as Arc<dyn TrackLocal>)
            .await
            .context("adicionar a trilha de áudio")?;

        Ok((
            Self {
                connection,
                screen,
                audio,
                sender,
                audio_sender,
                payload_type: 0,
                audio_payload_type: 0,
                frame_rate,
            },
            receptor,
        ))
    }

    pub async fn create_offer(&self) -> Result<String> {
        let oferta = self
            .connection
            .create_offer(None)
            .await
            .context("criar oferta")?;

        self.connection
            .set_local_description(oferta.clone())
            .await
            .context("aplicar oferta local")?;

        Ok(oferta.sdp)
    }

    pub async fn accept_offer(&mut self, sdp: String) -> Result<String> {
        let oferta = RTCSessionDescription::offer(sdp).context("montar oferta")?;

        self.connection
            .set_remote_description(oferta)
            .await
            .context("aplicar oferta remota")?;

        let resposta = self
            .connection
            .create_answer(None)
            .await
            .context("criar resposta")?;

        self.connection
            .set_local_description(resposta.clone())
            .await
            .context("aplicar resposta local")?;
        self.resolve_payload_type().await?;

        Ok(resposta.sdp)
    }

    pub async fn accept_answer(&mut self, sdp: String) -> Result<()> {
        let resposta = RTCSessionDescription::answer(sdp).context("montar resposta")?;

        self.connection
            .set_remote_description(resposta)
            .await
            .context("aplicar resposta")?;
        self.resolve_payload_type().await?;

        Ok(())
    }

    pub async fn add_candidate(&self, json: String) -> Result<()> {
        let candidato = serde_json::from_str(&json).context("ler candidato")?;

        self.connection
            .add_ice_candidate(candidato)
            .await
            .context("adicionar candidato")?;

        Ok(())
    }

    /// O payload type sai da negociação, não é escolhido por nós: cada pacote precisa
    /// carregar exatamente o que foi acordado no SDP.
    async fn resolve_payload_type(&mut self) -> Result<()> {
        self.payload_type = negociado(&self.sender)
            .await
            .ok_or_else(|| anyhow!("a outra ponta não aceitou H.264"))?;

        // Áudio é opcional: se a outra ponta não quiser Opus, o vídeo continua.
        self.audio_payload_type = negociado(&self.audio_sender).await.unwrap_or(0);

        Ok(())
    }

    /// Envia um bloco Opus já comprimido.
    pub async fn send_audio(&self, opus: &[u8]) -> Result<()> {
        if self.audio_payload_type == 0 {
            return Ok(());
        }

        self.audio
            .sample_writer(SSRC_AUDIO, self.audio_payload_type)
            .write_sample(&Sample {
                data: opus.to_vec().into(),
                duration: Duration::from_millis(crate::audio::FRAME_MS as u64),
                ..Default::default()
            })
            .await
            .context("enviar áudio")?;

        Ok(())
    }

    /// Envia um quadro já comprimido. A duração diz ao empacotador quanto tempo o
    /// quadro ocupa — errar aqui faz o vídeo acelerar ou arrastar.
    pub async fn send_frame(&self, frame: &EncodedFrame) -> Result<()> {
        self.screen
            .sample_writer(SSRC_VIDEO, self.payload_type)
            .write_sample(&Sample {
                data: frame.data.clone().into(),
                duration: Duration::from_secs_f64(1.0 / self.frame_rate),
                ..Default::default()
            })
            .await
            .context("enviar quadro")?;

        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        self.connection.close().await.context("fechar conexão")?;

        Ok(())
    }
}

fn trilha(
    id: &str,
    rotulo: &str,
    kind: RtpCodecKind,
    ssrc: u32,
    codec: RTCRtpCodec,
) -> MediaStreamTrack {
    MediaStreamTrack::new(
        "discord2".to_owned(),
        id.to_owned(),
        rotulo.to_owned(),
        kind,
        vec![RTCRtpEncodingParameters {
            rtp_coding_parameters: RTCRtpCodingParameters {
                ssrc: Some(ssrc),
                ..Default::default()
            },
            codec,
            ..Default::default()
        }],
    )
}

/// O payload type sai da negociação, não é escolhido por nós: cada pacote precisa
/// carregar exatamente o que foi acordado no SDP.
async fn negociado(sender: &Arc<dyn RtpSender>) -> Option<PayloadType> {
    sender
        .get_parameters()
        .await
        .ok()?
        .rtp_parameters
        .codecs
        .first()
        .map(|codec| codec.payload_type)
}
