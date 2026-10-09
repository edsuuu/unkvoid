//! Decodificador de H.264 no Linux: um `gst-launch` por transmissão, com o `avdec_h264` que o
//! `.deb` já exige (`gstreamer1.0-libav`). É o par do `windows_decoder.rs`, com a mesma API,
//! para o app Slint assistir igual nos dois sistemas.
//!
//! O quadro entra pela entrada padrão e sai RGBA cru pela saída, cada ponta numa thread: o
//! `decode` nunca espera o GStreamer, e devolve a imagem mais nova que ficou pronta desde a
//! última chamada.
//!
//! ponytail: a saída é 1280x720 fixa, com tarja preta para a proporção, porque o RGBA cru pelo
//! cano não diz o tamanho do quadro e um tamanho fixo dispensa procurar separador. Decodificar
//! e converter são na CPU. Teto: uma tela 1080p60 custa um núcleo de quem assiste e chega com
//! menos detalhe. A saída é o pipeline dentro do processo (`gstreamer-rs`), com o `vah264dec`
//! e o tamanho de verdade nas caps.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use anyhow::{Context, Result, anyhow};

use crate::DecodedFrame;

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

const FRAME_BYTES: usize = WIDTH as usize * HEIGHT as usize * 4;

/// Quadros esperando o GStreamer. Passou disso ele não está acompanhando, e quem chama
/// recomeça no próximo keyframe, como faz quando o MFT recusa.
const WAITING: usize = 16;

/// Quadros prontos esperando o `decode`. Quem desenha só quer o mais novo.
const READY: usize = 4;

pub struct H264Decoder {
    child: Child,
    feed: SyncSender<Vec<u8>>,
    ready: Receiver<Vec<u8>>,
}

impl H264Decoder {
    pub fn new() -> Result<Self> {
        let mut child = Command::new("gst-launch-1.0")
            .arg("-q")
            .args(pipeline().split_whitespace())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("gst-launch-1.0 não abriu; instale gstreamer1.0-tools e gstreamer1.0-libav")?;
        let input = child.stdin.take().context("o gst-launch abriu sem entrada")?;
        let output = child.stdout.take().context("o gst-launch abriu sem saída")?;
        let (feed, blocks) = sync_channel::<Vec<u8>>(WAITING);
        let (done, ready) = sync_channel(READY);

        write_blocks(input, blocks);
        read_frames(output, done);

        // Sem isto, um decodificador que morre (falta o `avdec_h264`) deixa só o cartão preto,
        // sem uma linha no log dizendo por quê.
        if let Some(errors) = child.stderr.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(errors).lines().map_while(Result::ok) {
                    tracing::warn!(%line, "assistir: gst");
                }
            });
        }

        Ok(Self { child, feed, ready })
    }

    /// Entrega um quadro Annex-B e devolve a imagem mais nova que ficou pronta, se ficou. O
    /// primeiro quadro costuma sair só numa chamada seguinte: `None` não é erro.
    pub fn decode(&mut self, annex_b: &[u8], _timestamp: u32) -> Result<Option<DecodedFrame>> {
        Ok(self.feed(annex_b)?.map(|rgba| DecodedFrame { width: WIDTH, height: HEIGHT, rgba }))
    }

    /// O mesmo, escrevendo a imagem no buffer que `target` der para o tamanho dela — o da
    /// imagem da interface. Diz se saiu imagem.
    pub fn decode_into<'target>(
        &mut self,
        annex_b: &[u8],
        _timestamp: u32,
        target: impl FnOnce(u32, u32) -> &'target mut [u8],
    ) -> Result<bool> {
        let Some(rgba) = self.feed(annex_b)? else {
            return Ok(false);
        };
        let slot = target(WIDTH, HEIGHT);

        if slot.len() != rgba.len() {
            return Err(anyhow!("a imagem da interface tem {} bytes, e o quadro {}", slot.len(), rgba.len()));
        }

        slot.copy_from_slice(&rgba);

        Ok(true)
    }

    /// Decodifica sem entregar imagem: o quadro passa pelo decodificador para o seguinte sair
    /// certo, e o que ficou pronto vai fora, porque um mais novo vai substituí-lo.
    pub fn skip(&mut self, annex_b: &[u8], _timestamp: u32) -> Result<()> {
        self.feed(annex_b).map(|_| ())
    }

    fn feed(&mut self, annex_b: &[u8]) -> Result<Option<Vec<u8>>> {
        if let Some(status) = self.child.try_wait()? {
            return Err(anyhow!("o gst-launch do vídeo terminou ({status})"));
        }

        match self.feed.try_send(annex_b.to_vec()) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(anyhow!("o decodificador não está acompanhando")),
            Err(TrySendError::Disconnected(_)) => return Err(anyhow!("o gst-launch do vídeo fechou a entrada")),
        }

        Ok(self.ready.try_iter().last())
    }
}

impl Drop for H264Decoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// H.264 Annex-B na entrada, RGBA no tamanho fixo na saída. O `typefind` é o que dá ao
/// `h264parse` o tipo que a entrada padrão não traz.
fn pipeline() -> String {
    format!(
        "fdsrc fd=0 ! typefind ! h264parse ! avdec_h264 thread-type=slice ! videoconvert ! videoscale \
         ! video/x-raw,format=RGBA,width={WIDTH},height={HEIGHT},pixel-aspect-ratio=1/1 ! fdsink fd=1 sync=false"
    )
}

fn write_blocks(mut input: impl Write + Send + 'static, blocks: Receiver<Vec<u8>>) {
    std::thread::spawn(move || {
        for block in blocks {
            if input.write_all(&block).is_err() {
                break;
            }
        }
    });
}

/// Cada `FRAME_BYTES` é um quadro inteiro: o pipeline força largura, altura e formato. Com a
/// fila de prontos cheia o quadro novo é largado, e quem desenha pega o seguinte.
fn read_frames(mut output: impl Read + Send + 'static, done: SyncSender<Vec<u8>>) {
    std::thread::spawn(move || {
        loop {
            let mut rgba = vec![0_u8; FRAME_BYTES];

            if output.read_exact(&mut rgba).is_err() {
                break;
            }

            if let Err(TrySendError::Disconnected(_)) = done.try_send(rgba) {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn the_pipeline_hands_back_raw_pixels_of_the_fixed_size() {
        let words = pipeline();

        assert!(words.starts_with("fdsrc fd=0 ! typefind ! h264parse ! avdec_h264"), "{words}");
        assert!(words.contains("format=RGBA,width=1280,height=720,pixel-aspect-ratio=1/1"), "{words}");
    }

    #[test]
    fn only_whole_frames_come_out() {
        let (reader, mut writer) = std::io::pipe().expect("um cano");
        let (done, ready) = sync_channel(READY);

        read_frames(reader, done);
        writer.write_all(&vec![7_u8; FRAME_BYTES + 10]).expect("um quadro e um pedaço");
        drop(writer);

        let frame = ready.recv_timeout(Duration::from_secs(5)).expect("um quadro inteiro");

        assert_eq!(frame.len(), FRAME_BYTES);
        assert!(ready.recv_timeout(Duration::from_millis(200)).is_err(), "o pedaço virou quadro");
    }

    /// Contra o GStreamer de verdade: o arquivo de teste entra e sai como quadros do tamanho
    /// fixo. Roda com `cargo test -p media -- --ignored`, numa máquina com o gst-launch.
    #[test]
    #[ignore]
    fn a_real_stream_comes_out_as_frames_of_the_fixed_size() {
        let stream = include_bytes!("../tests/fixtures/testsrc-320x240.h264");
        let mut decoder = H264Decoder::new().expect("o gst-launch abriu");
        let mut frame = decoder.decode(stream, 0).expect("entrou");
        let deadline = Instant::now() + Duration::from_secs(10);

        while frame.is_none() {
            assert!(Instant::now() < deadline, "nenhum quadro saiu");
            std::thread::sleep(Duration::from_millis(20));
            frame = decoder.decode(&[], 0).expect("continua vivo");
        }

        let frame = frame.expect("um quadro");

        assert_eq!((frame.width, frame.height, frame.rgba.len()), (WIDTH, HEIGHT, FRAME_BYTES));

        let mut image = vec![0_u8; FRAME_BYTES];

        decoder.skip(&[], 0).expect("pular continua vivo");
        decoder.decode_into(stream, 0, |_, _| &mut image).expect("o quadro entra de novo");
    }
}
