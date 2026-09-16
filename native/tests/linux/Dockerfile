# O Linux que a gente entrega: Ubuntu com os mesmos pacotes que o .deb exige.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV CARGO_TERM_COLOR=never
ENV PATH=/root/.cargo/bin:$PATH

# Os plugins do GStreamer são os mesmos nomes que o `depends` do .deb lista, mais o
# Xvfb (uma tela de mentira) e o PulseAudio (um microfone de mentira).
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl pkg-config cmake \
        gstreamer1.0-tools \
        gstreamer1.0-plugins-base \
        gstreamer1.0-plugins-good \
        gstreamer1.0-plugins-bad \
        gstreamer1.0-plugins-ugly \
        gstreamer1.0-libav \
        gstreamer1.0-x \
        gstreamer1.0-pulseaudio \
        xvfb x11-xserver-utils x11-utils x11-apps \
        pulseaudio pulseaudio-utils \
    && rm -rf /var/lib/apt/lists/*

# O que o Tauri precisa para compilar e empacotar no Linux — é com isto que sai o .deb.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-dev \
        libgtk-3-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libsoup-3.0-dev \
        libssl-dev \
        libglib2.0-dev \
        patchelf file desktop-file-utils fuse3 \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /unkvoid/native
