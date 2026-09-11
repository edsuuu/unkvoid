#!/usr/bin/env bash
#
# Prova o .deb numa Debian 12 limpa (Dockerfile.test), em segundos: instala, `--version`
# e três segundos de captura + encoder sob Xvfb (`--check-capture`). Não está no caminho
# da publicação — quem valida de verdade é quem instala; isto é para olhar antes.
#
#   ./validate-deb.sh caminho/para/unkvoid_x.y.z_amd64.deb
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

DEB=$(readlink -f "$1")

docker build -q -t unkvoid-linux-test -f Dockerfile.test . > /dev/null

docker run --rm -v "$DEB:/tmp/unkvoid.deb:ro" unkvoid-linux-test bash -c '
set -e
apt-get install -y -qq /tmp/unkvoid.deb 2>&1 | tail -3
unkvoid-desktop --version
# Xvfb na mão: o xvfb-run da Debian 12 fica esperando para sempre quando o comando
# termina antes de ele achar que o servidor subiu.
(Xvfb :77 -screen 0 1280x800x24 -nolisten tcp > /dev/null 2>&1 &)
export DISPLAY=:77 WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 HOME=/root
for i in $(seq 1 50); do xdpyinfo > /dev/null 2>&1 && break; sleep 0.1; done
timeout -s KILL 25 unkvoid-desktop --check-capture
' 2>&1 | grep -vE "^\s*$|Gtk-WARNING|Gdk-WARNING|dbind|Fontconfig|libEGL|MESA|glx"
