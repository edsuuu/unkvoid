# Compila, testa e abre o app Slint no Linux sem precisar de uma máquina Linux.
#
# O `xvfb` dá uma tela que ninguém vê, e o Mesa (`llvmpipe`) um OpenGL por software para o
# `femtovg` do Slint desenhar nela — é o mesmo caminho de uma máquina sem placa. Quem quiser
# o renderizador por software do Slint, sem OpenGL nenhum, passa `SLINT_BACKEND=winit-software`.
FROM rust:1-trixie

# `libgstreamer1.0-dev` e `libgstreamer-plugins-base1.0-dev` são a captura do Linux, que roda o
# GStreamer dentro do processo (`gstreamer-rs`): sem eles o `gstreamer-sys` morre no `pkg-config`.
# `libdbus-1-dev` é do chaveiro (`shared/storage` fala Secret Service). `libfontconfig1-dev`,
# `libxkbcommon-dev` e `libwayland-dev` são o que o `winit` e o `femtovg` pedem para compilar
# no Linux; `libegl1`, `libgl1-mesa-dri` e `libgles2` são o que eles abrem em tempo de
# execução. O GStreamer e o `pactl`/`pacat` são a mídia: sem eles o app abre, mas não
# transmite nem toca nada — a lista é a mesma do `.deb`.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libgstreamer1.0-dev \
        libgstreamer-plugins-base1.0-dev \
        libdbus-1-dev \
        libfontconfig1-dev \
        libxkbcommon-dev \
        libwayland-dev \
        libegl1 \
        libgles2 \
        libgl1-mesa-dri \
        cmake \
        pkg-config \
        xvfb \
        xauth \
        x11-apps \
        x11-xserver-utils \
        x11-utils \
        imagemagick \
        fonts-dejavu-core \
        gstreamer1.0-tools \
        gstreamer1.0-plugins-base \
        gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad \
        gstreamer1.0-plugins-ugly \
        gstreamer1.0-libav \
        gstreamer1.0-x \
        gstreamer1.0-pulseaudio \
        pulseaudio \
        pulseaudio-utils \
    && rm -rf /var/lib/apt/lists/*

# O `check` do repo cobra clippy, e a imagem base do Rust não o traz.
RUN rustup component add clippy

WORKDIR /app

# Só o que o app precisa: o Tauri e o GTK não entram, e trazê-los faria o contêiner compilar
# meia hora de coisa que este teste não usa.
COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY apps/windows ./apps/windows
COPY apps/desktop/src-tauri/icons ./apps/desktop/src-tauri/icons

# O workspace da máquina inclui membros que não existem aqui.
RUN printf '[workspace]\nresolver = "3"\nmembers = ["shared/storage", "shared/core", "shared/capture", "shared/media", "apps/windows"]\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"\nlicense = "MIT"\n\n[workspace.dependencies]\nanyhow = "1"\nserde = { version = "1", features = ["derive"] }\nserde_json = "1"\nthiserror = "2"\ntokio = { version = "1", features = ["full"] }\ntracing = "0.1"\ntracing-subscriber = { version = "0.3", features = ["env-filter"] }\n' > Cargo.toml

# `-l` não: o `/etc/profile` do Debian reescreve o PATH e esconde o cargo da imagem.
CMD ["bash", "-c", "cargo test -p unkvoid-windows && cargo clippy -p unkvoid-windows --all-targets -- -D warnings && xvfb-run -a cargo run -p unkvoid-windows"]
