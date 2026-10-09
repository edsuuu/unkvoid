//! A sala viva de ponta a ponta, contra o SFU de verdade: uma pessoa transmite e outra assiste,
//! no mesmo processo, e quem assiste decodifica e marca a hora de cada imagem como o app faz
//! (`media::Playout` e o `H264Decoder` do sistema). Prova o caminho inteiro — captura, encoder,
//! RTP/SRTP, SFU, recuperação, remontagem, decodificador — sem janela nenhuma.
//!
//! Precisa de um SFU no ar, de uma tela X com algo mexendo e de um servidor de som com saída
//! padrão; o `native/shared/core/tests/ponta-a-ponta.sh` monta tudo isso e roda estes testes:
//!
//! ```bash
//! UNKVOID_SFU=ws://127.0.0.1:3000/sfu cargo test -p core-app --test live_room -- --ignored --test-threads=1
//! ```
//!
//! O de sincronia espera, na tela e no som, o vídeo de um clarão branco com um bipe de 1 kHz a
//! cada segundo (o script o gera), e mede como o app toca: a imagem na hora do `Playout`, o som
//! na chegada mais a espera da tela que ele acompanha (`Speaker::hold`). Com `UNKVOID_LOSS=3` a
//! espera da imagem cresce, e o som tem de crescer junto. O da câmera pede
//! `UNKVOID_CAMERA_SOURCE`; o de mover de canal, o `SFU_SECRET` do SFU; os do quadro-chave de quem
//! entra e da perda, `UNKVOID_KEYFRAME_SECONDS=4` (o GOP do Windows, no build de depuração).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_app::Identity;
use core_app::models::RoomIdentity;
use core_app::room::Room;
use core_app::watching::{Media, MediaKind};
use serde_json::Value;

fn sfu() -> String {
    std::env::var("UNKVOID_SFU").unwrap_or_else(|_| "ws://127.0.0.1:3000/sfu".into())
}

/// Um código de sala novo por teste: o mesmo código entre dois testes acharia a sala do anterior.
fn code(prefix: &str) -> String {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() % 1_000_000;

    format!("{prefix}{stamp:06}")
}

fn guest(room: &str, name: &str) -> Identity {
    let (room, name) = (room.to_owned(), name.to_owned());

    Arc::new(move || {
        let identity = RoomIdentity::Guest { room: room.clone(), name: name.clone(), install_id: format!("ponta-a-ponta-{name}") };

        Box::pin(async move { Ok(identity) }) as _
    })
}

/// Os avisos da sala, guardados para o teste procurar depois.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<Value>>>);

impl Heard {
    fn listen(updates: Receiver<String>) -> Self {
        let heard = Self::default();
        let kept = heard.clone();

        std::thread::spawn(move || {
            for update in updates {
                if let Ok(value) = serde_json::from_str::<Value>(&update) {
                    kept.0.lock().expect("os avisos").push(value);
                }
            }
        });

        heard
    }

    fn any(&self, wanted: impl Fn(&Value) -> bool) -> bool {
        self.0.lock().expect("os avisos").iter().any(wanted)
    }
}

async fn enter(room: &str, name: &str) -> (Arc<Room>, Receiver<Media>, Heard) {
    let (updates, heard) = std::sync::mpsc::channel();
    let (room, media) = Room::enter(&sfu(), room, guest(room, name), updates).await.expect("entrou na sala");

    (room, media, Heard::listen(heard))
}

fn share_config(fps: u32) -> capture::CaptureConfig {
    core_app::sharing::capture_config(&serde_json::json!({ "quality": "720", "fps": fps, "audio": true, "muteCalls": false }))
}

/// O que quem assiste viu de uma transmissão de vídeo.
#[derive(Default)]
struct Picture {
    images: Vec<(Instant, u32, u32, f64)>,
    /// O relógio do RTP e a chegada de cada quadro, para conferir que ele anda com o tempo.
    clock: Vec<(u32, Instant)>,
}

/// O que quem assiste ouviu de uma transmissão de som: quando cada bloco toca e o volume dele, e
/// o vídeo que o núcleo diz que ele acompanha.
#[derive(Default)]
struct Sound {
    blocks: Vec<(Instant, f32, Option<usize>)>,
    follows: Option<String>,
}

#[derive(Default)]
struct Seen {
    pictures: HashMap<String, Picture>,
    sounds: HashMap<String, Sound>,
}

/// Quem assiste: esvazia a fila da sala numa thread, decodifica cada tela no horário do
/// `Playout` e mede o som, como a thread de cada tela do app. O som que acompanha uma tela toca
/// com a espera dela, como o `Speaker::hold` do app.
struct Viewer {
    seen: Arc<Mutex<Seen>>,
    stop: Arc<AtomicBool>,
}

impl Viewer {
    fn start(media: Receiver<Media>) -> Self {
        let (seen, stop) = (Arc::new(Mutex::new(Seen::default())), Arc::new(AtomicBool::new(false)));

        std::thread::spawn({
            let (seen, stop) = (Arc::clone(&seen), Arc::clone(&stop));

            move || {
                let mut decoders: HashMap<String, (media::H264Decoder, media::Playout)> = HashMap::new();
                let mut delays: HashMap<String, Duration> = HashMap::new();

                while !stop.load(Ordering::Relaxed) {
                    let item = match media.recv_timeout(Duration::from_millis(100)) {
                        Ok(item) => item,
                        Err(RecvTimeoutError::Timeout) => continue,
                        Err(RecvTimeoutError::Disconnected) => return,
                    };

                    match item.kind {
                        MediaKind::Video { keyframe, timestamp } => {
                            seen.lock().expect("o visto").pictures.entry(item.producer_id.clone()).or_default().clock.push((timestamp, item.arrived));

                            if !decoders.contains_key(&item.producer_id) {
                                if !keyframe {
                                    continue;
                                }

                                decoders.insert(item.producer_id.clone(), (media::H264Decoder::new().expect("o decodificador abriu"), media::Playout::default()));
                            }

                            let Some((decoder, playout)) = decoders.get_mut(&item.producer_id) else {
                                continue;
                            };
                            let due = playout.due(timestamp, item.arrived);

                            delays.insert(item.producer_id.clone(), playout.delay());

                            let started = Instant::now();
                            let Ok(Some(image)) = decoder.decode(&item.data, timestamp) else {
                                continue;
                            };
                            let shown = due.max(item.arrived) + started.elapsed();

                            seen.lock().expect("o visto").pictures.entry(item.producer_id).or_default().images.push((
                                shown,
                                image.width,
                                image.height,
                                brightness(&image.rgba),
                            ));
                        }
                        MediaKind::Audio => {
                            let samples: Vec<f32> = item.data.as_chunks::<4>().0.iter().map(|bytes| f32::from_le_bytes(*bytes)).collect();
                            let loud = samples.iter().position(|sample| sample.abs() > 0.2).map(|index| index / 2);
                            let rms = (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len().max(1) as f32).sqrt();

                            let hold = item.follows.as_ref().and_then(|screen| delays.get(screen)).copied().unwrap_or_default();
                            let mut seen = seen.lock().expect("o visto");
                            let sound = seen.sounds.entry(item.producer_id).or_default();

                            sound.follows.clone_from(&item.follows);
                            sound.blocks.push((item.arrived + hold, rms, loud));
                        }
                    }
                }
            }
        });

        Self { seen, stop }
    }

    fn images(&self, producer: &str) -> usize {
        self.seen.lock().expect("o visto").pictures.get(producer).map_or(0, |picture| picture.images.len())
    }

    fn frames(&self, producer: &str) -> usize {
        self.seen.lock().expect("o visto").pictures.get(producer).map_or(0, |picture| picture.clock.len())
    }

    fn blocks(&self, producer: &str) -> usize {
        self.seen.lock().expect("o visto").sounds.get(producer).map_or(0, |sound| sound.blocks.len())
    }

    /// Espera até `wanted` imagens da transmissão, ou o prazo.
    fn wait_images(&self, producer: &str, wanted: usize, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;

        while Instant::now() < deadline {
            if self.images(producer) >= wanted {
                return true;
            }

            std::thread::sleep(Duration::from_millis(50));
        }

        false
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// O brilho médio de uma amostra dos pixels, de 0 a 255.
fn brightness(rgba: &[u8]) -> f64 {
    let picked: Vec<f64> = rgba.chunks_exact(4).step_by(97).map(|pixel| f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2])).collect();

    picked.iter().sum::<f64>() / picked.len().max(1) as f64 / 3.0
}

/// A tela e o som dela, do jeito que o cartão do app os acha: o `audio` do cartão da tela.
fn screen_of(room: &Room) -> Option<(String, Option<String>)> {
    let tiles = room.tiles();

    tiles["tiles"].as_array()?.iter().find(|tile| tile["camera"] == false && tile["mine"] == false).map(|tile| {
        (tile["producerId"].as_str().unwrap_or_default().to_owned(), tile["audio"].as_str().map(str::to_owned))
    })
}

fn wait_for<T>(patience: Duration, mut found: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + patience;

    while Instant::now() < deadline {
        if let Some(value) = found() {
            return Some(value);
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    None
}

/// As imagens por segundo de uma transmissão numa janela, pela hora em que cada uma apareceu.
fn rate(seen: &Seen, producer: &str, from: Instant, to: Instant) -> f64 {
    let shown = seen.pictures.get(producer).map_or(0, |picture| picture.images.iter().filter(|(at, ..)| *at >= from && *at < to).count());

    shown as f64 / to.duration_since(from).as_secs_f64()
}

/// A maior parada entre duas imagens seguidas numa janela.
fn longest_pause(seen: &Seen, producer: &str, from: Instant, to: Instant) -> Duration {
    let Some(picture) = seen.pictures.get(producer) else {
        return to.duration_since(from);
    };
    let mut times: Vec<Instant> = picture.images.iter().map(|(at, ..)| *at).filter(|at| *at >= from && *at < to).collect();

    times.sort();
    times.windows(2).map(|pair| pair[1].duration_since(pair[0])).max().unwrap_or(to.duration_since(from))
}

/// Quem chega depois vê a tela sem esperar o quadro-chave periódico, e ouve o som dela junto:
/// cada clarão da imagem é casado com o bipe mais perto dele.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do SFU, de uma tela X e de um servidor de som — ver o topo do arquivo"]
async fn a_late_viewer_sees_and_hears_the_screen_in_sync() {
    let code = code("tela");
    let (sharer, _own, _) = enter(&code, "quem-mostra").await;

    sharer.share(share_config(30)).await.expect("compartilhou");

    // Quem chega depois: a imagem tem de aparecer sem esperar o quadro-chave periódico.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let (viewer_room, media, _) = enter(&code, "quem-assiste").await;
    let joined = Instant::now();
    let viewer = Viewer::start(media);
    let (screen, audio) = tokio::task::block_in_place(|| wait_for(Duration::from_secs(5), || screen_of(&viewer_room))).expect("o cartão da tela apareceu");
    let audio = audio.expect("a tela veio com o som dela");

    // O som da tela chega mudo por regra, e quem assiste o liga no cartão.
    viewer_room.mute_watched(&audio, false);

    assert!(tokio::task::block_in_place(|| viewer.wait_images(&screen, 1, Duration::from_secs(3))), "a primeira imagem não chegou em 3 s ({} quadros chegaram)", viewer.frames(&screen));

    let first = viewer.seen.lock().expect("o visto").pictures[&screen].images[0].0;

    tokio::time::sleep(Duration::from_secs(12)).await;

    {
        let now = Instant::now();
        let seen = viewer.seen.lock().expect("o visto");
        let picture = &seen.pictures[&screen];
        let sound = seen.sounds.get(&audio).unwrap_or_else(|| {
            panic!("o som da tela não chegou: {audio} calado={:?}; chegou som de {:?}", viewer_room.is_watched_muted(&audio), seen.sounds.keys().collect::<Vec<_>>())
        });
        let fps = rate(&seen, &screen, now - Duration::from_secs(10), now);
        let pause = longest_pause(&seen, &screen, now - Duration::from_secs(10), now);
        let (_, width, height, _) = *picture.images.last().expect("uma imagem");

        let flashes: Vec<Instant> = picture.images.windows(2).filter(|pair| pair[0].3 < 100.0 && pair[1].3 > 150.0).map(|pair| pair[1].0).collect();
        let beeps: Vec<Instant> = sound
            .blocks
            .windows(2)
            .filter(|pair| pair[0].1 < 0.05 && pair[1].1 >= 0.05)
            .map(|pair| pair[1].0 + Duration::from_secs_f64(pair[1].2.unwrap_or(0) as f64 / 48_000.0))
            .collect();
        let mut offsets: Vec<f64> = flashes
            .iter()
            .filter_map(|flash| {
                beeps
                    .iter()
                    .map(|beep| flash.duration_since(*beep).as_secs_f64() * 1000.0 - beep.duration_since(*flash).as_secs_f64() * 1000.0)
                    .min_by(|left, right| left.abs().total_cmp(&right.abs()))
            })
            .filter(|offset| offset.abs() < 500.0)
            .collect();

        offsets.sort_by(f64::total_cmp);

        let median = offsets.get(offsets.len() / 2).copied().unwrap_or(f64::NAN);

        println!(
            "primeira imagem {} ms depois de entrar; {fps:.1} imagens/s; maior parada {} ms; {width}x{height}; {} blocos de som; desvio A/V (imagem − som) em ms: {offsets:?}, mediana {median:.0}",
            first.duration_since(joined).as_millis(),
            pause.as_millis(),
            sound.blocks.len(),
        );

        assert!(first.duration_since(joined) < Duration::from_secs(3), "quem chegou depois esperou demais pela primeira imagem");
        assert!(fps >= 25.0, "só {fps:.1} imagens por segundo");
        assert!(pause < Duration::from_millis(500), "a imagem parou {} ms", pause.as_millis());
        assert_eq!((width, height), (1280, 720));
        assert!(sound.blocks.len() > 400, "só {} blocos de som em 12 s", sound.blocks.len());
        assert_eq!(sound.follows.as_ref(), Some(&screen), "o som da tela não diz que acompanha a tela");
        assert!(offsets.len() >= 5, "poucos clarões casados com bipes: {offsets:?} (clarões {}, bipes {})", flashes.len(), beeps.len());
        assert!(median.abs() < 80.0, "a imagem está {median:.0} ms fora do som");
    }

    viewer_room.leave().await;
    sharer.leave().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do SFU — ver o topo do arquivo"]
async fn the_microphone_reaches_the_room() {
    let code = code("voz");
    let (speaker, _own, _) = enter(&code, "quem-fala").await;
    let (listener_room, media, _) = enter(&code, "quem-ouve").await;
    let listener = Viewer::start(media);

    speaker.open_microphone().await.expect("o microfone abriu");

    let tone: Vec<f32> = (0..960).flat_map(|index| {
        let sample = (index as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin() * 0.5;

        [sample, sample]
    }).collect();
    let started = Instant::now();

    while started.elapsed() < Duration::from_secs(3) {
        speaker.speak(&tone);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let peers = listener_room.peers();
    let mic = peers["peers"]
        .as_array()
        .and_then(|peers| peers.iter().flat_map(|peer| peer["producers"].as_array().cloned().unwrap_or_default()).find(|producer| producer["source"] == "mic"))
        .and_then(|producer| producer["producerId"].as_str().map(str::to_owned))
        .expect("o microfone apareceu na sala");
    let heard = listener.seen.lock().expect("o visto").sounds.get(&mic).map(|sound| sound.blocks.iter().filter(|(_, rms, _)| *rms > 0.1).count()).unwrap_or(0);

    println!("{heard} blocos de voz com som de {} recebidos", listener.blocks(&mic));

    assert!(heard > 50, "só {heard} blocos com voz em 3 s");

    listener_room.leave().await;
    speaker.leave().await;
}

/// Só o Linux captura a câmera pelo núcleo; no Windows ela ainda não existe, e no macOS vem da
/// interface (`open_camera`).
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do SFU, de uma tela X, de um servidor de som e de UNKVOID_CAMERA_SOURCE — ver o topo do arquivo"]
async fn a_camera_is_watched_beside_the_screen() {
    assert!(std::env::var_os("UNKVOID_CAMERA_SOURCE").is_some(), "sem UNKVOID_CAMERA_SOURCE não há câmera neste teste");

    let code = code("camera");
    let (sharer, _own, _) = enter(&code, "quem-mostra").await;
    let (viewer_room, media, _) = enter(&code, "quem-assiste").await;
    let viewer = Viewer::start(media);

    sharer.share(share_config(30)).await.expect("compartilhou");
    sharer
        .open_captured_camera(capture::CaptureConfig { source: capture::CaptureSource::Camera(0), capture_audio: false, ..capture::CaptureConfig::default() })
        .await
        .expect("a câmera abriu");

    let camera = tokio::task::block_in_place(|| {
        wait_for(Duration::from_secs(5), || {
            viewer_room.tiles()["tiles"].as_array()?.iter().find(|tile| tile["camera"] == true).and_then(|tile| tile["producerId"].as_str().map(str::to_owned))
        })
    })
    .expect("o cartão da câmera apareceu");
    let (screen, _) = screen_of(&viewer_room).expect("o cartão da tela continua");

    assert!(tokio::task::block_in_place(|| viewer.wait_images(&camera, 60, Duration::from_secs(5))), "a câmera não deu 60 imagens em 5 s");
    assert!(viewer.images(&screen) > 30, "a tela parou quando a câmera entrou");

    let size = viewer.seen.lock().expect("o visto").pictures[&camera].images.last().map(|(_, width, height, _)| (*width, *height));

    assert_eq!(size, Some((640, 360)));

    sharer.close_captured_camera().await;

    assert!(
        tokio::task::block_in_place(|| wait_for(Duration::from_secs(3), || viewer_room.tiles()["tiles"].as_array()?.iter().all(|tile| tile["camera"] == false).then_some(()))).is_some(),
        "o cartão da câmera não saiu"
    );

    viewer_room.leave().await;
    sharer.leave().await;
}

/// Entra na sala e espera a primeira imagem da tela e da câmera de quem transmite: quanto cada
/// uma levou desde antes de entrar (o clique), ou `None` se não veio em 4 s.
async fn late_join(code: &str, name: &str) -> (String, Option<Duration>, Option<Duration>) {
    let started = Instant::now();
    let (room, media, _) = enter(code, name).await;
    let viewer = Viewer::start(media);
    let first = |camera: bool| {
        let tiles = room.tiles();
        let producer = tiles["tiles"].as_array()?.iter().find(|tile| tile["camera"] == camera && tile["mine"] == false)?["producerId"].as_str()?.to_owned();
        let seen = viewer.seen.lock().expect("o visto");

        seen.pictures.get(&producer)?.images.first().map(|(shown, ..)| shown.duration_since(started))
    };
    let (mut screen, mut camera) = (None, None);

    while started.elapsed() < Duration::from_secs(4) && (screen.is_none() || camera.is_none()) {
        screen = screen.or_else(|| first(false));
        camera = camera.or_else(|| first(true));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    room.leave().await;

    (name.to_owned(), screen, camera)
}

/// Quem entra atrasado vê a tela e a câmera em até 1 s só pelo pedido de quadro-chave, também
/// quem entra 300 ms depois de outra pessoa. É o cenário `b` do harness do SFU (#54) com o
/// cliente nativo de verdade, e o GOP de 4 s do encoder do Windows (`UNKVOID_KEYFRAME_SECONDS=4`):
/// com o freio de 2 s o segundo atrasado esperava ~2 s, e com o PLI lido por quem lia primeiro a
/// câmera esperava o GOP.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do SFU, de uma tela X, de UNKVOID_CAMERA_SOURCE e de UNKVOID_KEYFRAME_SECONDS=4 — ver o topo do arquivo"]
async fn late_viewers_see_the_screen_and_the_camera_within_a_second() {
    assert_eq!(std::env::var("UNKVOID_KEYFRAME_SECONDS").as_deref(), Ok("4"), "com o GOP de 1 s o quadro-chave periódico chega antes do pedido, e o teste não prova nada");
    assert!(std::env::var_os("UNKVOID_CAMERA_SOURCE").is_some(), "sem UNKVOID_CAMERA_SOURCE não há câmera neste teste");

    let code = code("atrasado");
    let (sharer, _own, _) = enter(&code, "quem-mostra").await;

    sharer.share(share_config(30)).await.expect("compartilhou");
    sharer
        .open_captured_camera(capture::CaptureConfig { source: capture::CaptureSource::Camera(0), capture_audio: false, ..capture::CaptureConfig::default() })
        .await
        .expect("a câmera abriu");
    tokio::time::sleep(Duration::from_secs(3)).await;

    let mut joins = Vec::new();

    // Um sozinho; depois dois a 300 ms um do outro; depois três juntos.
    joins.push(late_join(&code, "a1").await);
    tokio::time::sleep(Duration::from_millis(1_500)).await;

    let first = tokio::spawn({
        let code = code.clone();

        async move { late_join(&code, "p1").await }
    });

    tokio::time::sleep(Duration::from_millis(300)).await;
    joins.push(late_join(&code, "p2").await);
    joins.push(first.await.expect("p1"));
    tokio::time::sleep(Duration::from_millis(1_500)).await;

    let together: Vec<_> = ["g1", "g2", "g3"].map(|name| tokio::spawn({
        let code = code.clone();

        async move { late_join(&code, name).await }
    })).into_iter().collect();

    for join in together {
        joins.push(join.await.expect("g"));
    }

    let late: Vec<String> = joins
        .iter()
        .flat_map(|(name, screen, camera)| [("tela", screen), ("câmera", camera)].map(|(what, took)| (name, what, *took)))
        .filter(|(_, _, took)| took.is_none_or(|took| took > Duration::from_secs(1)))
        .map(|(name, what, took)| format!("{name} {what}: {took:?}"))
        .collect();

    println!(
        "primeira imagem de quem entra atrasado (tela, câmera) em ms: {:?}",
        joins.iter().map(|(name, screen, camera)| (name.as_str(), screen.map(|took| took.as_millis()), camera.map(|took| took.as_millis()))).collect::<Vec<_>>()
    );

    assert!(late.is_empty(), "passaram de 1 s: {late:?}");

    sharer.leave().await;
}

/// 5% do que chega some (o `UNKVOID_LOSS` do receptor), por 15 s, com o GOP de 4 s: a imagem não
/// para mais de 1 s. É o cenário `c` (descida) do harness do SFU com o cliente nativo: sem o RR o
/// mediasoup não reenviava de novo o pacote cujo reenvio se perdeu, o buraco virava PLI, e o PLI
/// esperava o freio de 2 s — de 3,6 a 7,2 s parado em 15 s.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do SFU, de uma tela X e de UNKVOID_KEYFRAME_SECONDS=4 — ver o topo do arquivo"]
async fn five_percent_lost_on_the_way_in_never_holds_the_picture_for_a_second() {
    assert_eq!(std::env::var("UNKVOID_KEYFRAME_SECONDS").as_deref(), Ok("4"), "com o GOP de 1 s o periódico tapa o buraco antes do pedido, e o teste não prova nada");

    let code = code("perda");
    let (sharer, _own, _) = enter(&code, "quem-mostra").await;

    sharer.share(share_config(30)).await.expect("compartilhou");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // SAFETY: lido pelo receptor de quem assiste, que é o único que abre enquanto ela vale; quem
    // transmite não assiste nada.
    unsafe { std::env::set_var("UNKVOID_LOSS", "5") };

    let (viewer_room, media, _) = enter(&code, "quem-assiste").await;
    let viewer = Viewer::start(media);
    let (screen, _) = tokio::task::block_in_place(|| wait_for(Duration::from_secs(5), || screen_of(&viewer_room))).expect("o cartão da tela apareceu");

    assert!(tokio::task::block_in_place(|| viewer.wait_images(&screen, 1, Duration::from_secs(3))), "a primeira imagem não chegou");

    // SAFETY: o mesmo; o receptor já leu.
    unsafe { std::env::remove_var("UNKVOID_LOSS") };

    let from = Instant::now();

    tokio::time::sleep(Duration::from_secs(15)).await;

    let to = Instant::now();
    let (fps, pause) = {
        let seen = viewer.seen.lock().expect("o visto");

        (rate(&seen, &screen, from, to), longest_pause(&seen, &screen, from, to))
    };

    println!("5% de perda na chegada por 15 s: {fps:.1} imagens/s, maior parada {} ms; {:?}", pause.as_millis(), viewer_room.counters(&screen));

    assert!(pause <= Duration::from_secs(1), "a imagem parou {} ms", pause.as_millis());
    assert!(fps >= 24.0, "só {fps:.1} imagens por segundo");

    viewer_room.leave().await;
    sharer.leave().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do SFU, de uma tela X e de um servidor de som — ver o topo do arquivo"]
async fn stopping_and_sharing_again_brings_the_picture_back() {
    let code = code("denovo");
    let (sharer, _own, _) = enter(&code, "quem-mostra").await;
    let (viewer_room, media, _) = enter(&code, "quem-assiste").await;
    let viewer = Viewer::start(media);

    sharer.share(share_config(30)).await.expect("compartilhou");

    let (first, _) = tokio::task::block_in_place(|| wait_for(Duration::from_secs(5), || screen_of(&viewer_room))).expect("a primeira tela");

    assert!(tokio::task::block_in_place(|| viewer.wait_images(&first, 30, Duration::from_secs(4))));

    sharer.stop_sharing().await;

    assert!(
        tokio::task::block_in_place(|| wait_for(Duration::from_secs(3), || screen_of(&viewer_room).is_none().then_some(()))).is_some(),
        "o cartão da tela parada não saiu"
    );

    sharer.share(share_config(30)).await.expect("compartilhou de novo");

    let again = Instant::now();
    let (second, _) = tokio::task::block_in_place(|| wait_for(Duration::from_secs(5), || screen_of(&viewer_room))).expect("a segunda tela");

    assert_ne!(first, second, "a tela nova é outro producer");
    assert!(tokio::task::block_in_place(|| viewer.wait_images(&second, 30, Duration::from_secs(4))), "a tela de novo não deu imagem");
    println!("a imagem voltou {} ms depois de compartilhar de novo", again.elapsed().as_millis());

    viewer_room.leave().await;
    sharer.leave().await;
}

/// Trocar o fps no meio refaz captura e encoder no mesmo producer: a imagem continua, e o
/// relógio do RTP anda o tempo de verdade que a troca levou — não volta, nem anda um quadro só.
/// O descompasso é a pior diferença, de um quadro para o seguinte, entre o que o relógio do RTP
/// andou e o que o de cá andou: a rede local não segura nada, então é a troca que aparece nele.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do SFU, de uma tela X e de um servidor de som — ver o topo do arquivo"]
async fn a_quality_change_keeps_the_clock_and_the_picture() {
    let code = code("troca");
    let (sharer, _own, _) = enter(&code, "quem-mostra").await;
    let (viewer_room, media, _) = enter(&code, "quem-assiste").await;
    let viewer = Viewer::start(media);

    sharer.share(share_config(30)).await.expect("compartilhou");

    let (screen, _) = tokio::task::block_in_place(|| wait_for(Duration::from_secs(5), || screen_of(&viewer_room))).expect("a tela");

    assert!(tokio::task::block_in_place(|| viewer.wait_images(&screen, 60, Duration::from_secs(5))));

    let changed = Instant::now();

    sharer.change_quality(capture::Quality::Hd720, 15, None).await.expect("trocou");
    tokio::time::sleep(Duration::from_secs(4)).await;

    {
        let now = Instant::now();
        let seen = viewer.seen.lock().expect("o visto");
        let picture = &seen.pictures[&screen];
        let pause = longest_pause(&seen, &screen, changed - Duration::from_secs(1), now);
        let fps = rate(&seen, &screen, now - Duration::from_secs(2), now);
        let drift = picture
            .clock
            .windows(2)
            .map(|pair| {
                let media = f64::from(pair[1].0.wrapping_sub(pair[0].0).cast_signed()) / 90_000.0;
                let wall = pair[1].1.duration_since(pair[0].1).as_secs_f64();

                ((wall - media) * 1000.0).abs()
            })
            .fold(0.0_f64, f64::max);

        println!("depois da troca: {fps:.1} imagens/s, maior parada {} ms, pior descompasso do relógio {drift:.0} ms", pause.as_millis());

        assert!(pause < Duration::from_millis(1_500), "a imagem parou {} ms na troca", pause.as_millis());
        assert!((12.0..=18.0).contains(&fps), "{fps:.1} imagens/s depois de pedir 15");
        assert!(drift < 150.0, "o relógio do RTP descompassou {drift:.0} ms na troca");
    }

    viewer_room.leave().await;
    sharer.leave().await;
}

/// Um moderador move quem está transmitindo: a sala de origem acaba para ela sem voltar
/// sozinha, a transmissão para, o aviso diz para onde ir, e a interface entra na sala nova e
/// transmite de novo, com quem assistia indo junto.
/// O `kick` com destino é o que o Laravel chama ao mover (`docs/CONTRATO.md`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "precisa do SFU com SFU_SECRET, de uma tela X e de um servidor de som — ver o topo do arquivo"]
async fn a_moved_person_leaves_for_good_and_shares_in_the_new_room() {
    let secret = std::env::var("SFU_SECRET").expect("o SFU_SECRET do SFU");
    let (origin, destination) = (code("origem"), code("destino"));
    let (moved, _own, heard) = enter(&origin, "quem-e-movido").await;
    let (viewer_room, media, _) = enter(&origin, "quem-assiste").await;
    let viewer = Viewer::start(media);

    moved.share(share_config(30)).await.expect("compartilhou");

    let (screen, _) = tokio::task::block_in_place(|| wait_for(Duration::from_secs(5), || screen_of(&viewer_room))).expect("a tela");

    assert!(tokio::task::block_in_place(|| viewer.wait_images(&screen, 30, Duration::from_secs(4))));

    let channel = "01jabcdefghjkmnpqrstvwxyz0";
    let body = serde_json::json!({ "userId": "guest:ponta-a-ponta-quem-e-movido", "to": channel, "by": "moderador" }).to_string();
    let path = format!("/rooms/{origin}/kick");
    let answer = signed_post(&secret, &path, &body);

    assert!(answer.contains("\"kicked\":1"), "o SFU não moveu ninguém: {answer}");

    let told = tokio::task::block_in_place(|| {
        wait_for(Duration::from_secs(3), || heard.any(|event| event["event"] == "room.session" && event["data"]["state"] == "moved" && event["data"]["to"] == channel).then_some(()))
    });

    assert!(told.is_some(), "a interface não soube que foi movida, nem para onde");
    assert!(
        tokio::task::block_in_place(|| wait_for(Duration::from_secs(3), || screen_of(&viewer_room).is_none().then_some(()))).is_some(),
        "a tela de quem foi movido ficou na sala de origem"
    );
    assert_eq!(moved.mine()["sharing"], false, "quem foi movido continuou transmitindo");

    tokio::time::sleep(Duration::from_secs(3)).await;

    assert!(!heard.any(|event| event["event"] == "room.session" && event["data"]["state"] == "rejoined"), "quem foi movido voltou sozinho para a origem");

    let (arrived, _arrived_media, _) = enter(&destination, "quem-e-movido").await;
    let (follower, follower_media, _) = enter(&destination, "quem-segue").await;
    let following = Viewer::start(follower_media);

    arrived.share(share_config(30)).await.expect("compartilhou no destino");

    let (new_screen, _) = tokio::task::block_in_place(|| wait_for(Duration::from_secs(5), || screen_of(&follower))).expect("a tela no destino");

    assert!(tokio::task::block_in_place(|| following.wait_images(&new_screen, 30, Duration::from_secs(4))), "a tela no destino não deu imagem");

    follower.leave().await;
    arrived.leave().await;
    viewer_room.leave().await;
}

/// O `POST` assinado do Laravel para o SFU, pelo `curl` e pelo `openssl` da máquina.
fn signed_post(secret: &str, path: &str, body: &str) -> String {
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs().to_string();
    let signed = format!("{timestamp}\nPOST\n{path}\n{body}");
    let mut openssl = std::process::Command::new("openssl")
        .args(["dgst", "-sha256", "-hmac", secret, "-hex"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("o openssl abriu");

    std::io::Write::write_all(&mut openssl.stdin.take().expect("a entrada"), signed.as_bytes()).expect("assinou");

    let digest = String::from_utf8(openssl.wait_with_output().expect("o openssl respondeu").stdout).expect("texto");
    let signature = digest.trim().rsplit(' ').next().unwrap_or_default().to_owned();
    let base = sfu().replace("ws://", "http://").replace("wss://", "https://");
    let origin = base.split("/sfu").next().unwrap_or_default().to_owned();
    let output = std::process::Command::new("curl")
        .args(["-s", "-X", "POST", "-H", "content-type: application/json"])
        .args(["-H", &format!("x-unkvoid-timestamp: {timestamp}"), "-H", &format!("x-unkvoid-signature: {signature}")])
        .args(["--data-binary", body, &format!("{origin}{path}")])
        .output()
        .expect("o curl respondeu");

    String::from_utf8_lossy(&output.stdout).into_owned()
}
