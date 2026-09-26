#!/usr/bin/env bash
#
# O .deb do app nativo do Linux, compilado dentro de um Debian 12 (Dockerfile.deb).
#
# O pacote se chama `unkvoid`, como o do Tauri: quem o tem recebe este no próximo
# `apt upgrade`, e o dpkg tira os arquivos do Tauri que este não traz. O `.desktop` mantém o
# nome `Unkvoid.desktop` para o atalho fixado no painel continuar valendo.
#
#   ./build-deb.sh                 # sai em native/target/deb12/Unkvoid_<versão>_amd64.deb
#   ./apt-publish.sh <o .deb>      # na VPS: ver docs/AUTO-UPDATE.md
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

NATIVE=$(cd ../.. && pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$NATIVE/Cargo.toml" | head -1)

if [ -z "${UNKVOID_IN_CONTAINER:-}" ]; then
    docker build -q -t unkvoid-linux-native -f Dockerfile.deb . > /dev/null

    # Tudo o que o build escreve fica dentro do `target/`, com o uid de quem chama. Um núcleo
    # de folga: com um rustc por núcleo, uma máquina de 4 GB (o WSL daqui) troca memória com
    # o disco até travar.
    CORES=$(nproc)

    exec docker run --rm --user "$(id -u):$(id -g)" -v "$NATIVE:/work" -w /work/apps/linux \
        -e UNKVOID_IN_CONTAINER=1 -e HOME=/work/target/deb12/home \
        -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-$(( CORES > 1 ? CORES - 1 : 1 ))}" \
        -e CARGO_HOME=/work/target/deb12/cargo-home -e CARGO_TARGET_DIR=/work/target/deb12 \
        unkvoid-linux-native ./build-deb.sh
fi

mkdir -p "$HOME"
nice -n 19 cargo build --release -p unkvoid-linux

ROOT=$(mktemp -d)/unkvoid
ICONS=../desktop/src-tauri/icons

install -Dm755 "$CARGO_TARGET_DIR/release/unkvoid" "$ROOT/usr/bin/unkvoid"
install -Dm644 "$ICONS/32x32.png" "$ROOT/usr/share/icons/hicolor/32x32/apps/com.unkvoid.desktop.png"
install -Dm644 "$ICONS/128x128.png" "$ROOT/usr/share/icons/hicolor/128x128/apps/com.unkvoid.desktop.png"
install -Dm644 "$ICONS/128x128@2x.png" "$ROOT/usr/share/icons/hicolor/256x256/apps/com.unkvoid.desktop.png"

# O ícone e a classe da janela têm o id do app (`APP_ID` no main.rs): é por ele que o painel
# reconhece a janela aberta como este atalho.
install -Dm644 /dev/stdin "$ROOT/usr/share/applications/Unkvoid.desktop" << DESKTOP
[Desktop Entry]
Type=Application
Name=Unkvoid
Comment=Compartilhar a tela sem perder fps
Exec=unkvoid
Icon=com.unkvoid.desktop
StartupWMClass=com.unkvoid.desktop
Categories=Network;AudioVideo;
Terminal=false
DESKTOP

# As bibliotecas saem do próprio binário; o resto são programas que o app chama: o
# `gst-launch-1.0` com os plugins da captura e do encoder, o `xrandr` e o `xdpyinfo` das telas
# no X11, e o `pactl` do som.
LIBRARIES=$(cd "$(mktemp -d)" && mkdir debian && touch debian/control \
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
Depends: $LIBRARIES, gstreamer1.0-tools, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good, gstreamer1.0-plugins-bad, gstreamer1.0-libav, gstreamer1.0-x264 | gstreamer1.0-plugins-ugly, gstreamer1.0-pipewire, gstreamer1.0-pulseaudio | gstreamer1.0-plugins-good, x11-xserver-utils, x11-utils, pulseaudio-utils
Recommends: gstreamer1.0-vaapi, xdg-desktop-portal
Description: Compartilhar a tela sem perder fps
 Unkvoid: um nome, um codigo de sala, e a tela. Captura nativa, encoder da placa de video,
 sem barra de navegador.
CONTROL

DEB="$CARGO_TARGET_DIR/Unkvoid_${VERSION}_amd64.deb"

dpkg-deb --root-owner-group --build "$ROOT" "$DEB" > /dev/null
echo "[INFO] $DEB"
