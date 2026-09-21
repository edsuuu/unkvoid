# Compila e roda o app Linux sem precisar de uma máquina Linux.
#
# O GTK4 não existe no macOS: este contêiner é o que permite verificar de verdade que o
# app compila e abre. `xvfb` dá a ele uma tela que ninguém vê — é o suficiente para provar
# que a janela sobe e que a ligação com o núcleo funciona.
FROM rust:1-trixie

# `libdbus-1-dev` porque o chaveiro do `shared/storage` fala Secret Service, e ele fala D-Bus.
# O GStreamer e o `pactl` porque o vídeo, o som e a escolha de aparelho passam por processos
# filhos: sem eles o app compila e abre, mas não transmite nem toca nada. A lista é a mesma
# de `tests/linux/Dockerfile` — `bad` traz o `h264parse` e `ugly` o `x264enc`.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libgtk-4-dev \
        libadwaita-1-dev \
        libdbus-1-dev \
        cmake \
        pkg-config \
        xvfb \
        xauth \
        gstreamer1.0-tools \
        gstreamer1.0-plugins-base \
        gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad \
        gstreamer1.0-plugins-ugly \
        gstreamer1.0-libav \
        gstreamer1.0-x \
        gstreamer1.0-pulseaudio \
        pulseaudio-utils \
    && rm -rf /var/lib/apt/lists/*

# O `check` do repo cobra clippy, e a imagem base do Rust não o traz.
RUN rustup component add clippy

WORKDIR /app

# Só o que o app Linux precisa: o Tauri não entra, e trazê-lo faria o contêiner compilar
# meia hora de coisa que este teste não usa. A captura e a mídia entram: são elas que
# transmitem e que fazem o vídeo dos outros aparecer.
COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY apps/linux ./apps/linux

# O workspace da máquina inclui membros que não existem aqui.
RUN printf '[workspace]\nresolver = "3"\nmembers = ["shared/storage", "shared/core", "shared/capture", "shared/media", "apps/linux"]\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"\nlicense = "MIT"\n\n[workspace.dependencies]\nanyhow = "1"\nserde = { version = "1", features = ["derive"] }\nserde_json = "1"\nthiserror = "2"\ntokio = { version = "1", features = ["full"] }\ntracing = "0.1"\ntracing-subscriber = { version = "0.3", features = ["env-filter"] }\n' > Cargo.toml

CMD ["bash", "-lc", "cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && xvfb-run -a cargo run -p unkvoid-linux"]
