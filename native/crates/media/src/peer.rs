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

/// What must reach the other side through SFU signaling.
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

/// Direct connection to another participant.
///
/// P2P is advantageous here: the server is in the US and users are in Brazil —
/// 139 ms round trip through it versus ~20 ms directly. Above 3 viewers the
/// broadcaster's upload multiplies and the SFU becomes worthwhile again.
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

        // H.264 because that is what the hardware encoder provides. Another codec
        // would require software encoding, putting the load back on the CPU.
        let codec = RTCRtpCodec {
            mime_type: MIME_TYPE_H264.to_owned(),
            clock_rate: 90_000,
            channels: 0,
            sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                .to_owned(),
            ..Default::default()
        };

        // System audio in Opus. Capture already excludes our own app's sound, so
        // there is no risk of sending a caller's voice back.
        let codec_audio = RTCRtpCodec {
            mime_type: MIME_TYPE_OPUS.to_owned(),
            clock_rate: crate::audio::SAMPLE_RATE,
            channels: crate::audio::CHANNELS,
            sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
            ..Default::default()
        };

        // The transceiver accepts only codecs known to the media engine: without
        // registering them here, adding the track fails with "unsupported codec type".
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
            .context("build screen track")?,
        );

        let audio = Arc::new(
            TrackLocalStaticSample::new(trilha(
                "audio",
                "som",
                RtpCodecKind::Audio,
                SSRC_AUDIO,
                codec_audio,
            ))
            .context("build audio track")?,
        );

        let sender = connection
            .add_track(Arc::clone(&screen) as Arc<dyn TrackLocal>)
            .await
            .context("add video track")?;

        let audio_sender = connection
            .add_track(Arc::clone(&audio) as Arc<dyn TrackLocal>)
            .await
            .context("add audio track")?;

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
            .context("create offer")?;

        self.connection
            .set_local_description(oferta.clone())
            .await
            .context("apply local offer")?;

        Ok(oferta.sdp)
    }

    pub async fn accept_offer(&mut self, sdp: String) -> Result<String> {
        let oferta = RTCSessionDescription::offer(sdp).context("build offer")?;

        self.connection
            .set_remote_description(oferta)
            .await
            .context("apply remote offer")?;

        let resposta = self
            .connection
            .create_answer(None)
            .await
            .context("create answer")?;

        self.connection
            .set_local_description(resposta.clone())
            .await
            .context("apply local answer")?;
        self.resolve_payload_type().await?;

        Ok(resposta.sdp)
    }

    pub async fn accept_answer(&mut self, sdp: String) -> Result<()> {
        let resposta = RTCSessionDescription::answer(sdp).context("build answer")?;

        self.connection
            .set_remote_description(resposta)
            .await
            .context("apply answer")?;
        self.resolve_payload_type().await?;

        Ok(())
    }

    pub async fn add_candidate(&self, json: String) -> Result<()> {
        let candidato = serde_json::from_str(&json).context("read candidate")?;

        self.connection
            .add_ice_candidate(candidato)
            .await
            .context("add candidate")?;

        Ok(())
    }

    /// The payload type comes from negotiation, not from us: each packet must carry
    /// exactly what was agreed in the SDP.
    async fn resolve_payload_type(&mut self) -> Result<()> {
        self.payload_type = negociado(&self.sender)
            .await
            .ok_or_else(|| anyhow!("the other side did not accept H.264"))?;

        // Audio is optional: if the other side does not want Opus, video continues.
        self.audio_payload_type = negociado(&self.audio_sender).await.unwrap_or(0);

        Ok(())
    }

    /// Sends an already-compressed Opus block.
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
            .context("send audio")?;

        Ok(())
    }

    /// Sends an already-compressed frame. Duration tells the packetizer how long the
    /// frame lasts — getting it wrong makes video speed up or drag.
    pub async fn send_frame(&self, frame: &EncodedFrame) -> Result<()> {
        self.screen
            .sample_writer(SSRC_VIDEO, self.payload_type)
            .write_sample(&Sample {
                data: frame.data.clone().into(),
                duration: Duration::from_secs_f64(1.0 / self.frame_rate),
                ..Default::default()
            })
            .await
            .context("send frame")?;

        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        self.connection.close().await.context("close connection")?;

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

/// The payload type comes from negotiation, not from us: each packet must carry
/// exactly what was agreed in the SDP.
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
