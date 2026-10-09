#!/usr/bin/env bash
#
# O .deb do app Slint no Linux, compilado dentro de um Debian 12 (Dockerfile.deb).
#
# O pacote se chama `unkvoid`, como o do Tauri e o do GTK: quem tem um deles recebe este no
# próximo `apt upgrade`, e o dpkg tira os arquivos que este não traz. O `.desktop` mantém o nome
# `Unkvoid.desktop` para o atalho fixado no painel continuar valendo.
#
#   ./build-deb.sh                 # sai em native/target/deb12/Unkvoid_<versão>_amd64.deb
#   ./apt-publish.sh <o .deb>      # na VPS: ver docs/AUTO-UPDATE.md
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

NATIVE=$(cd ../.. && pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$NATIVE/Cargo.toml" | head -1)

if [ -z "${UNKVOID_IN_CONTAINER:-}" ]; then
    docker build -q -t unkvoid-windows-deb -f Dockerfile.deb . > /dev/null

    # Tudo o que o build escreve fica dentro do `target/`, com o uid de quem chama.
    exec docker run --rm --user "$(id -u):$(id -g)" -v "$NATIVE:/work" -w /work/apps/windows \
        -e UNKVOID_IN_CONTAINER=1 -e HOME=/work/target/deb12/home \
        -e CARGO_HOME=/work/target/deb12/cargo-home -e CARGO_TARGET_DIR=/work/target/deb12 \
        unkvoid-windows-deb ./build-deb.sh
fi

mkdir -p "$HOME"
nice -n 19 cargo build --release -p unkvoid-windows

ROOT=$(mktemp -d -p "$CARGO_TARGET_DIR")/unkvoid
ICONS=../desktop/src-tauri/icons

install -Dm755 "$CARGO_TARGET_DIR/release/unkvoid" "$ROOT/usr/bin/unkvoid"
install -Dm644 "$ICONS/32x32.png" "$ROOT/usr/share/icons/hicolor/32x32/apps/com.unkvoid.desktop.png"
install -Dm644 "$ICONS/128x128.png" "$ROOT/usr/share/icons/hicolor/128x128/apps/com.unkvoid.desktop.png"
install -Dm644 "$ICONS/128x128@2x.png" "$ROOT/usr/share/icons/hicolor/256x256/apps/com.unkvoid.desktop.png"

# O `StartupWMClass` é o nome do binário: é o que o `winit` escreve na classe da janela (X11) e
# no `app_id` (Wayland) quando ninguém manda outro, e é por ele que o painel reconhece a janela
# aberta como este atalho.
install -Dm644 /dev/stdin "$ROOT/usr/share/applications/Unkvoid.desktop" << DESKTOP
[Desktop Entry]
Type=Application
Name=Unkvoid
Comment=Compartilhar a tela sem perder fps
Exec=unkvoid
Icon=com.unkvoid.desktop
StartupWMClass=unkvoid
Categories=Network;AudioVideo;
Terminal=false
DESKTOP

# As bibliotecas que o binário liga de verdade saem do `dpkg-shlibdeps`; o `winit` e o `femtovg`
# abrem as deles em tempo de execução (`dlopen`), então entram à mão: `libxkbcommon0`,
# `libwayland-client0`, `libegl1`. O resto são programas que o app chama: o `gst-launch-1.0` com
# os plugins da captura, do encoder e do decoder, o `xrandr` e o `xdpyinfo` das telas no X11, e
# o `pactl`/`pacat` do som.
LIBRARIES=$(cd "$(mktemp -d -p "$CARGO_TARGET_DIR")" && mkdir debian && touch debian/control \
    && dpkg-shlibdeps -O "$ROOT/usr/bin/unkvoid" | sed 's/^shlibs:Depends=//')

mkdir -p "$ROOT/DEBIAN"
cat > "$ROOT/DEBIAN/control" << CONTROL
Package: unkvoid
Version: ${VERSION//-/\~}
Architecture: amd64
Maintainer: Unkvoid
Priority: optional
Section: net
Homepage: https://unkvoid.com
Installed-Size: $(du -sk "$ROOT/usr" | cut -f1)
Depends: $LIBRARIES, libxkbcommon0, libwayland-client0, libegl1, libgl1, fontconfig, gstreamer1.0-tools, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good, gstreamer1.0-plugins-bad, gstreamer1.0-libav, gstreamer1.0-x264 | gstreamer1.0-plugins-ugly, gstreamer1.0-pipewire, gstreamer1.0-pulseaudio | gstreamer1.0-plugins-good, x11-xserver-utils, x11-utils, pulseaudio-utils
Recommends: gstreamer1.0-vaapi, xdg-desktop-portal, libgl1-mesa-dri
Description: Compartilhar a tela sem perder fps
 Unkvoid: um nome, um codigo de sala, e a tela. Captura nativa, encoder da placa de video,
 sem barra de navegador.
CONTROL

DEB="$CARGO_TARGET_DIR/Unkvoid_${VERSION}_amd64.deb"

dpkg-deb --root-owner-group --build "$ROOT" "$DEB" > /dev/null
echo "[INFO] $DEB"
