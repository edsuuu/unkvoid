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

        while Instant::now() < until {
            let Ok(next) =
                tokio::task::block_in_place(|| media.recv_timeout(Duration::from_millis(200)))
            else {
                continue;
            };

            match next.kind {
                MediaKind::Video { keyframe, .. } => {
                    frames += 1;
                    keyframes += u32::from(keyframe);
                    video_bytes += next.data.len();
                }
                MediaKind::Audio => audio_blocks += 1,
            }
        }

        println!(
            "chegou: {frames} quadros ({keyframes} keyframes, {video_bytes} bytes) e {audio_blocks} blocos de som"
        );
    }

    room.leave().await;

    Ok(())
}
