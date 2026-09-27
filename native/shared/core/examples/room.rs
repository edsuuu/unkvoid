//! A sala viva de ponta a ponta, sem interface: um lado compartilha, o outro conta o que chega.
//!
//! ```bash
//! cargo run -p core-app --example room -- ws://127.0.0.1:3000/sfu sala-de-teste share
//! cargo run -p core-app --example room -- ws://127.0.0.1:3000/sfu sala-de-teste watch
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use core_app::models::RoomIdentity;
use core_app::room::Room;
use core_app::watching::MediaKind;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let mut arguments = std::env::args().skip(1);
    let url = arguments
        .next()
        .unwrap_or_else(|| "ws://127.0.0.1:3000/sfu".into());
    let code = arguments.next().unwrap_or_else(|| "sala-de-teste".into());
    let sharing = arguments.next().as_deref() == Some("share");
    let seconds: u64 = arguments
        .next()
        .and_then(|text| text.parse().ok())
        .unwrap_or(10);

    let identity = {
        let (room, name) = (
            code.clone(),
            if sharing {
                "quem-mostra"
            } else {
                "quem-assiste"
            },
        );

        Arc::new(move || {
            let identity = RoomIdentity::Guest {
                room: room.clone(),
                name: name.into(),
                install_id: format!("example-{name}"),
            };

            Box::pin(async move { Ok(identity) }) as _
        })
    };

    let (updates, heard) = std::sync::mpsc::channel();
    let (room, media) = Room::enter(&url, &code, identity, updates).await?;

    std::thread::spawn(move || {
        for update in heard {
            if !update.contains("room.level") {
                println!("aviso: {update}");
            }
        }
    });

    if sharing {
        room.share(core_app::sharing::capture_config(
            &serde_json::json!({ "quality": "720", "fps": 30 }),
        ))
        .await?;
        println!("compartilhando por {seconds} s: {}", room.mine());
        tokio::time::sleep(Duration::from_secs(seconds)).await;
    } else {
        let until = Instant::now() + Duration::from_secs(seconds);
        let (mut frames, mut keyframes, mut video_bytes, mut audio_blocks) =
            (0_u32, 0_u32, 0_usize, 0_u32);
        let mut screens = std::collections::BTreeSet::new();
        // Uma linha por segundo e as pausas entre quadros: é assim que congelamento aparece.
        let started = Instant::now();
        let (mut second, mut second_frames, mut second_keyframes) = (0_u64, 0_u32, 0_u32);
        let (mut last_frame, mut longest_pause, mut pauses) = (None::<Instant>, Duration::ZERO, 0_u32);
        let mut counted = (0_u64, 0_u64, 0_u64);

        while Instant::now() < until {
            let now_second = started.elapsed().as_secs();

            if now_second != second {
                let (received, recovered, lost) = screens
                    .iter()
                    .filter_map(|screen: &String| room.counters(screen))
                    .fold((0, 0, 0), |sum, counters| {
                        (sum.0 + counters.received, sum.1 + counters.recovered, sum.2 + counters.lost)
                    });

                println!(
                    "t={second:>3}s fps={second_frames:>3} kf={second_keyframes} pacotes={} recuperados={} perdidos={}",
                    received - counted.0,
                    recovered - counted.1,
                    lost - counted.2
                );
                counted = (received, recovered, lost);
                (second, second_frames, second_keyframes) = (now_second, 0, 0);
            }

            let Ok(next) =
                tokio::task::block_in_place(|| media.recv_timeout(Duration::from_millis(200)))
            else {
                continue;
            };

            match next.kind {
                MediaKind::Video { keyframe, .. } => {
                    let arrived = Instant::now();

                    if let Some(previous) = last_frame {
                        let pause = arrived.duration_since(previous);

                        longest_pause = longest_pause.max(pause);
                        pauses += u32::from(pause > Duration::from_millis(250));
                    }

                    last_frame = Some(arrived);
                    screens.insert(next.producer_id.clone());
                    frames += 1;
                    second_frames += 1;
                    keyframes += u32::from(keyframe);
                    second_keyframes += u32::from(keyframe);
                    video_bytes += next.data.len();
                }
                MediaKind::Audio => audio_blocks += 1,
            }
        }

        println!("pausas acima de 250 ms: {pauses}, a maior: {} ms", longest_pause.as_millis());

        println!(
            "chegou: {frames} quadros ({keyframes} keyframes, {video_bytes} bytes) e {audio_blocks} blocos de som"
        );

        for screen in &screens {
            if let Some(counters) = room.counters(screen) {
                println!(
                    "pacotes da tela {screen}: {} recebidos, {} recuperados por reenvio, {} perdidos",
                    counters.received, counters.recovered, counters.lost
                );
            }
        }
    }

    room.leave().await;

    Ok(())
}
