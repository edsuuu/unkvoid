#!/usr/bin/env bash
#
# Prova um `.deb` numa Debian 12 limpa. A instalação do pacote puxa as dependências declaradas,
# então um erro aqui mostra que o pacote não se sustenta fora da máquina que o construiu.
#
#   native/release/validate-deb.sh caminho/para/unkvoid_x.y.z_amd64.deb
set -euo pipefail

SCRIPT_DIRECTORY=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PACKAGE_FILE=$(readlink -f "${1:?informe o .deb}")

docker build -q -t unkvoid-package-test -f "$SCRIPT_DIRECTORY/Dockerfile.test" "$SCRIPT_DIRECTORY" > /dev/null

docker run --rm -v "$PACKAGE_FILE:/tmp/unkvoid.deb:ro" unkvoid-package-test bash -c '
set -e
apt-get install -y -qq /tmp/unkvoid.deb 2>&1 | tail -3
unkvoid --version
'
