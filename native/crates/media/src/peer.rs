use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rtc::interceptor::Registry;
use rtc::media::Sample;
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::RTCConfigurationBuilder;
use rtc::peer_connection::configuration::interceptor_registry::register_default_interceptors;
use rtc::peer_connection::configuration::media_engine::{MIME_TYPE_H264, MediaEngine};
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

const SSRC: u32 = 0x1234_5678;

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
    sender: Arc<dyn RtpSender>,
    payload_type: PayloadType,
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
            TrackLocalStaticSample::new(MediaStreamTrack::new(
                "discord2".to_owned(),
                "screen".to_owned(),
                "tela".to_owned(),
                RtpCodecKind::Video,
                vec![RTCRtpEncodingParameters {
                    rtp_coding_parameters: RTCRtpCodingParameters {
                        ssrc: Some(SSRC),
                        ..Default::default()
                    },
                    codec: codec.clone(),
                    ..Default::default()
                }],
            ))
            .context("montar a trilha da tela")?,
        );

        let sender = connection
            .add_track(Arc::clone(&screen) as Arc<dyn TrackLocal>)
            .await
            .context("adicionar a trilha")?;

        Ok((
            Self {
                connection,
                screen,
                sender,
                payload_type: 0,
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
        self.payload_type = self
            .sender
            .get_parameters()
            .await
            .context("ler parâmetros do sender")?
            .rtp_parameters
            .codecs
            .first()
            .map(|codec| codec.payload_type)
            .ok_or_else(|| anyhow!("a outra ponta não aceitou H.264"))?;

        Ok(())
    }

    /// Envia um quadro já comprimido. A duração diz ao empacotador quanto tempo o
    /// quadro ocupa — errar aqui faz o vídeo acelerar ou arrastar.
    pub async fn send_frame(&self, frame: &EncodedFrame) -> Result<()> {
        self.screen
            .sample_writer(SSRC, self.payload_type)
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
