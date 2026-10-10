//! Decodificador de H.264 no Linux: o `avdec_h264` que o `.deb` já exige
//! (`gstreamer1.0-libav`), dentro do processo pelo `gstreamer-rs`. É o par do
//! `windows_decoder.rs`, com a mesma API, para o app Slint assistir igual nos dois sistemas.
//!
//! Dentro do processo, e não num `gst-launch` com o quadro atravessando um cano: o RGBA cru
//! do cano não diz o tamanho, e o tamanho fixo que isso exigia (1280x720 com tarja) jogava fora
//! o detalhe de uma tela 1080p; e o cano devolvia a imagem do quadro anterior, então a última
//! mudança de uma tela parada só aparecia no quadro seguinte — um segundo depois, no Windows,
//! que repete a imagem parada uma vez por segundo. Aqui cada quadro devolve a própria imagem,
//! no tamanho que veio, e a troca de resolução no meio chega nas caps do quadro.
//!
//! ponytail: decodificar e converter para RGBA são na CPU. Teto: uma tela 1080p60 custa perto
//! de um núcleo de quem assiste. A saída é o `vah264dec`/`nvh264dec` no lugar do `avdec_h264`,
//! com o `videoconvert` trocado pelo da placa.

use std::sync::Once;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use crate::DecodedFrame;

/// Quanto se espera pela imagem de um quadro. Um 4K na CPU leva dezenas de milissegundos; mais
/// que isto é o decodificador recusando o quadro (que ele larga com um aviso, sem imagem), e a
/// tela não pode parar esperando. A imagem que chegar depois é de um quadro velho e vai fora.
const PATIENCE: Duration = Duration::from_millis(250);

/// O H.264 entra quadro a quadro, em Annex-B, e sai RGBA no tamanho que veio. Sem `h264parse`:
/// o quadro já chega inteiro (`alignment=au`), e é assim que a marca de "só decodificar" do
/// `skip` chega intacta ao decodificador.
const PIPELINE: &str = "appsrc name=source format=time ! avdec_h264 thread-type=slice ! videoconvert \
     ! video/x-raw,format=RGBA ! appsink name=sink sync=false";

pub struct H264Decoder {
    pipeline: gst::Pipeline,
    source: gst_app::AppSrc,
    sink: gst_app::AppSink,
    /// Quantos quadros entraram. É o carimbo de cada um: a imagem que sai traz o carimbo do
    /// quadro de onde veio, e é assim que a de um quadro velho é reconhecida e largada.
    sent: u64,
}

impl H264Decoder {
    pub fn new() -> Result<Self> {
        init()?;

        let pipeline = gst::parse::launch(PIPELINE)
            .context("o decodificador não montou; instale o gstreamer1.0-libav")?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("o decodificador não é um pipeline"))?;
        let source = pipeline
            .by_name("source")
            .and_then(|element| element.downcast::<gst_app::AppSrc>().ok())
            .ok_or_else(|| anyhow!("o decodificador veio sem entrada"))?;
        let sink = pipeline
            .by_name("sink")
            .and_then(|element| element.downcast::<gst_app::AppSink>().ok())
            .ok_or_else(|| anyhow!("o decodificador veio sem saída"))?;

        source.set_caps(Some(
            &gst::Caps::builder("video/x-h264").field("stream-format", "byte-stream").field("alignment", "au").build(),
        ));

        pipeline.set_state(gst::State::Playing).context("o decodificador não começou")?;

        Ok(Self { pipeline, source, sink, sent: 0 })
    }

    /// Decodifica um quadro Annex-B e devolve a imagem dele. `None` é o quadro que o
    /// decodificador não transformou em imagem (recusado, ou a imagem demorou demais).
    pub fn decode(&mut self, annex_b: &[u8], _timestamp: u32) -> Result<Option<DecodedFrame>> {
        let Some(sample) = self.feed(annex_b, false)? else {
            return Ok(None);
        };
        let (width, height, rgba) = pixels(&sample)?;
        let mut copied = vec![0_u8; width as usize * height as usize * 4];

        copy_rows(rgba, &mut copied, width, height)?;

        Ok(Some(DecodedFrame { width, height, rgba: copied }))
    }

    /// O mesmo, escrevendo a imagem no buffer que `target` der para o tamanho dela — o da
    /// imagem da interface. Diz se saiu imagem.
    pub fn decode_into<'target>(
        &mut self,
        annex_b: &[u8],
        _timestamp: u32,
        target: impl FnOnce(u32, u32) -> &'target mut [u8],
    ) -> Result<bool> {
        let Some(sample) = self.feed(annex_b, false)? else {
            return Ok(false);
        };
        let (width, height, rgba) = pixels(&sample)?;

        copy_rows(rgba, target(width, height), width, height)?;

        Ok(true)
    }

    /// Decodifica sem imagem: o quadro passa pelo decodificador para o seguinte sair certo, e
    /// nem chega a ser convertido — a marca `DECODE_ONLY` faz o decodificador não entregá-lo.
    pub fn skip(&mut self, annex_b: &[u8], _timestamp: u32) -> Result<()> {
        self.feed(annex_b, true).map(|_| ())
    }

    fn feed(&mut self, annex_b: &[u8], decode_only: bool) -> Result<Option<gst::Sample>> {
        self.failure()?;

        let stamp = gst::ClockTime::from_mseconds(self.sent);
        let mut buffer = gst::Buffer::from_slice(annex_b.to_vec());

        {
            let buffer = buffer.get_mut().ok_or_else(|| anyhow!("o quadro não ficou só deste decodificador"))?;

            buffer.set_pts(stamp);

            if decode_only {
                buffer.set_flags(gst::BufferFlags::DECODE_ONLY);
            }
        }

        self.sent += 1;
        self.source.push_buffer(buffer).map_err(|flow| anyhow!("o decodificador recusou o quadro ({flow:?})"))?;

        if decode_only {
            return Ok(None);
        }

        loop {
            let Some(sample) = self.sink.try_pull_sample(gst::ClockTime::from_nseconds(PATIENCE.as_nanos() as u64)) else {
                self.failure()?;

                return Ok(None);
            };

            match sample.buffer().and_then(gst::BufferRef::pts) {
                Some(pts) if pts == stamp => return Ok(Some(sample)),
                Some(pts) if pts < stamp => {}
                _ => return Ok(Some(sample)),
            }
        }
    }

    /// O erro que o GStreamer anunciou: com ele o decodificador não volta, e quem chama abre
    /// outro no próximo keyframe.
    fn failure(&self) -> Result<()> {
        let Some(bus) = self.pipeline.bus() else {
            return Ok(());
        };

        match bus.pop_filtered(&[gst::MessageType::Error]) {
            Some(message) => match message.view() {
                gst::MessageView::Error(failure) => Err(anyhow!(
                    "o decodificador falhou: {} ({})",
                    failure.error(),
                    failure.debug().map(|debug| debug.to_string()).unwrap_or_default()
                )),
                _ => Ok(()),
            },
            None => Ok(()),
        }
    }
}

impl Drop for H264Decoder {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

fn init() -> Result<()> {
    static STARTED: Once = Once::new();
    let mut failure = None;

    STARTED.call_once(|| failure = gst::init().err());

    failure.map_or(Ok(()), |failure| Err(anyhow!("o GStreamer não iniciou: {failure}")))
}

/// Largura, altura e os bytes da imagem de um quadro, pelo que as caps dele dizem.
fn pixels(sample: &gst::Sample) -> Result<(u32, u32, gst::MappedBuffer<gst::buffer::Readable>)> {
    let caps = sample.caps().and_then(|caps| caps.structure(0)).ok_or_else(|| anyhow!("a imagem veio sem caps"))?;
    let width = caps.get::<i32>("width").context("a imagem veio sem largura")?;
    let height = caps.get::<i32>("height").context("a imagem veio sem altura")?;
    let buffer = sample.buffer_owned().ok_or_else(|| anyhow!("a imagem veio vazia"))?;
    let map = buffer.into_mapped_buffer_readable().map_err(|_| anyhow!("a imagem não abriu para leitura"))?;

    Ok((u32::try_from(width)?, u32::try_from(height)?, map))
}

/// Do buffer do GStreamer, que pode ter linhas mais largas que a imagem, para o da interface,
/// que não tem sobra nenhuma entre elas.
fn copy_rows(rgba: impl AsRef<[u8]>, target: &mut [u8], width: u32, height: u32) -> Result<()> {
    let rgba = rgba.as_ref();
    let row = width as usize * 4;
    let rows = height as usize;

    if rows == 0 || row == 0 {
        return Err(anyhow!("a imagem veio sem tamanho"));
    }

    let stride = rgba.len() / rows;

    if stride < row || target.len() != row * rows {
        return Err(anyhow!("a imagem tem {} bytes para {width}x{height}, e o destino {}", rgba.len(), target.len()));
    }

    for (line, out) in rgba.chunks_exact(stride).zip(target.chunks_exact_mut(row)) {
        out.copy_from_slice(&line[..row]);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Um fluxo H.264 de verdade, saído do `x264enc`, com o desenho `pattern` do `videotestsrc`.
    /// Os testes que o usam pedem os plugins do `.deb` (ugly e libav) e rodam com
    /// `cargo test -p media -- --ignored`.
    fn encoded_as(pattern: &str, width: u32, height: u32, frames: u32) -> Vec<Vec<u8>> {
        init().expect("o GStreamer iniciou");

        let description = format!(
            "videotestsrc pattern={pattern} num-buffers={frames} ! video/x-raw,format=I420,width={width},height={height},framerate=30/1 \
             ! x264enc tune=zerolatency speed-preset=ultrafast key-int-max=30 byte-stream=true \
             ! video/x-h264,stream-format=byte-stream,alignment=au,profile=constrained-baseline ! appsink name=sink sync=false"
        );
        let pipeline = gst::parse::launch(&description).expect("o encoder montou").downcast::<gst::Pipeline>().expect("um pipeline");
        let sink = pipeline.by_name("sink").and_then(|element| element.downcast::<gst_app::AppSink>().ok()).expect("a saída");

        pipeline.set_state(gst::State::Playing).expect("o encoder começou");

        let mut out = Vec::new();

        while let Ok(sample) = sink.pull_sample() {
            out.push(sample.buffer().expect("um quadro").map_readable().expect("legível").to_vec());
        }

        pipeline.set_state(gst::State::Null).expect("parou");

        out
    }

    fn encoded(width: u32, height: u32, frames: u32) -> Vec<Vec<u8>> {
        encoded_as("smpte", width, height, frames)
    }

    fn brightness(image: &DecodedFrame) -> u64 {
        image.rgba.chunks_exact(4).map(|pixel| u64::from(pixel[0]) + u64::from(pixel[1]) + u64::from(pixel[2])).sum::<u64>()
            / (u64::from(image.width) * u64::from(image.height))
    }

    /// A imagem que volta é a do quadro que entrou: um branco seguido de um preto devolve o
    /// branco e depois o preto. Pelo cano do `gst-launch` de antes, o preto devolvia o branco.
    #[test]
    #[ignore]
    fn the_image_that_comes_back_is_the_frame_that_went_in() {
        let white = encoded_as("white", 320, 240, 1);
        let black = encoded_as("black", 320, 240, 1);
        let mut decoder = H264Decoder::new().expect("o decodificador abriu");

        let first = decoder.decode(&white[0], 0).expect("decodificou").expect("o branco virou imagem");
        let second = decoder.decode(&black[0], 0).expect("decodificou").expect("o preto virou imagem");

        assert!(brightness(&first) > 600, "o branco saiu com brilho {}", brightness(&first));
        assert!(brightness(&second) < 100, "o preto saiu com brilho {}", brightness(&second));
    }

    #[test]
    #[ignore]
    fn each_frame_comes_back_as_its_own_image_in_its_own_size() {
        let frames = encoded(320, 240, 5);
        let mut decoder = H264Decoder::new().expect("o decodificador abriu");

        for (index, frame) in frames.iter().enumerate() {
            let image = decoder.decode(frame, 0).expect("decodificou").unwrap_or_else(|| panic!("o quadro {index} não virou imagem"));

            assert_eq!((image.width, image.height, image.rgba.len()), (320, 240, 320 * 240 * 4));
        }
    }

    /// Um tamanho fora da grade de 16 do H.264 (o 1080 de toda tela Full HD, o 1366 de notebook)
    /// sai no tamanho de verdade, sem a sobra do macrobloco: o encoder corta pelo SPS, e o
    /// decodificador tem de respeitar o corte. Ímpar o H.264 em 4:2:0 nem representa — quem
    /// transmite arredonda para par antes (`Quality::fit`).
    #[test]
    #[ignore]
    fn a_size_off_the_macroblock_grid_comes_back_whole() {
        let frames = encoded(322, 182, 2);
        let mut decoder = H264Decoder::new().expect("o decodificador abriu");
        let mut image = Vec::new();

        let drawn = decoder
            .decode_into(&frames[0], 0, |width, height| {
                image = vec![0; width as usize * height as usize * 4];
                &mut image
            })
            .expect("decodificou");

        assert!(drawn);
        assert_eq!(image.len(), 322 * 182 * 4);
        assert!(image.chunks_exact(4).any(|pixel| pixel != [0, 0, 0, 0]), "a imagem saiu vazia");
    }

    /// Quem transmite trocou a qualidade: o quadro-chave novo traz outro tamanho, e a imagem
    /// segue o tamanho novo sem abrir outro decodificador.
    #[test]
    #[ignore]
    fn the_size_follows_a_resolution_change_in_the_middle() {
        let mut decoder = H264Decoder::new().expect("o decodificador abriu");
        let small = encoded(320, 180, 2);
        let large = encoded(640, 360, 2);

        let first = decoder.decode(&small[0], 0).expect("decodificou").expect("imagem");
        let second = decoder.decode(&large[0], 0).expect("decodificou").expect("imagem");
        let third = decoder.decode(&large[1], 0).expect("decodificou").expect("imagem");

        assert_eq!((first.width, first.height), (320, 180));
        assert_eq!((second.width, second.height), (640, 360));
        assert_eq!((third.width, third.height), (640, 360));
    }

    /// O quadro pulado passa pelo decodificador sem imagem, e o seguinte sai com a dele.
    #[test]
    #[ignore]
    fn a_skipped_frame_gives_no_image_and_the_next_one_does() {
        let frames = encoded(320, 240, 3);
        let mut decoder = H264Decoder::new().expect("o decodificador abriu");

        decoder.skip(&frames[0], 0).expect("pulou");
        decoder.skip(&frames[1], 0).expect("pulou");

        assert!(decoder.decode(&frames[2], 0).expect("decodificou").is_some(), "o quadro depois dos pulados não virou imagem");
    }

    #[test]
    fn the_rows_lose_the_padding_and_a_short_buffer_is_refused() {
        let padded = [[1_u8; 4], [2; 4], [9; 4], [3; 4], [4; 4], [9; 4]].concat();
        let mut out = vec![0_u8; 16];

        copy_rows(&padded, &mut out, 2, 2).expect("copiou");

        assert_eq!(out, [[1_u8; 4], [2; 4], [3; 4], [4; 4]].concat());
        assert!(copy_rows(&padded[..8], &mut out, 2, 2).is_err());
    }
}
