#!/usr/bin/env bash
#
# Põe um .deb no repositório APT e refaz o índice assinado.
#
# O formato é o "plano": um diretório só, sem a árvore `dists/pool`. O APT aceita, e do
# lado de quem instala isso é uma linha de sources.list em vez de quatro.
#
#   ./apt-publish.sh caminho/para/Unkvoid_0.0.3_amd64.deb
#
# A chave que assina o índice é a `repo@unkvoid.com` do chaveiro desta máquina, e é
# OUTRA chave: a do auto-update assina o instalador, esta assina a lista de pacotes.
set -euo pipefail

REPO="${UNKVOID_APT:-/var/www/apt}"
DEB="${1:?informe o .deb}"
SIGNER="${UNKVOID_APT_KEY:-repo@unkvoid.com}"

if ! gpg --list-secret-keys "$SIGNER" > /dev/null 2>&1; then
    echo "[ERRO] sem a chave $SIGNER neste chaveiro — o índice sairia sem assinatura" >&2
    exit 1
fi

mkdir -p "$REPO"
cp -f "$DEB" "$REPO/"
cd "$REPO"

# `--multiversion` mantém as versões antigas na lista, para quem quiser fixar uma.
dpkg-scanpackages --multiversion . > Packages
gzip -9fk Packages

apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=Unkvoid \
    -o APT::FTPArchive::Release::Label=Unkvoid \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    release . > Release

# O `InRelease` é o Release com a assinatura dentro, e é o que o APT moderno procura
# primeiro. O `Release.gpg` fica para clientes antigos. Escrever num temporário e mover
# evita que um `apt update` no meio do caminho leia um arquivo pela metade.
gpg --batch --yes --clearsign --local-user "$SIGNER" -o InRelease.tmp Release
mv -f InRelease.tmp InRelease

gpg --batch --yes --detach-sign --armor --local-user "$SIGNER" -o Release.gpg.tmp Release
mv -f Release.gpg.tmp Release.gpg

echo "[INFO] repositório atualizado em $REPO"
ls -1 "$REPO"/*.deb | sed 's|.*/|  |'
