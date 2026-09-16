#!/usr/bin/env bash
#
# Põe um .deb no repositório APT e refaz o índice assinado.
#
# O formato é o "plano": um diretório só, sem a árvore `dists/pool`. O APT aceita, e do
# lado de quem instala isso é uma linha de sources.list em vez de quatro.
#
# Os arquivos não ficam no disco da VPS: vão para o bucket `apt` do MinIO, que é público
# para leitura e que o nginx expõe em https://unkvoid.com/apt/.
#
#   ./apt-publish.sh caminho/para/Unkvoid_0.0.3_amd64.deb
#
# A chave que assina o índice é a `repo@unkvoid.com` do chaveiro desta máquina, e é
# OUTRA chave: a do auto-update assina o instalador, esta assina a lista de pacotes.
set -euo pipefail

# O repositório mora num bucket público do MinIO, não no disco: o índice é montado num
# diretório temporário e sincronizado com o `mc`. O nginx serve o bucket em /apt/.
BUCKET="${UNKVOID_APT_BUCKET:-local/apt}"
DEB="${1:?informe o .deb}"
SIGNER="${UNKVOID_APT_KEY:-repo@unkvoid.com}"

if ! gpg --list-secret-keys "$SIGNER" > /dev/null 2>&1; then
    echo "[ERRO] sem a chave $SIGNER neste chaveiro — o índice sairia sem assinatura" >&2
    exit 1
fi

if ! command -v mc > /dev/null 2>&1; then
    echo "[ERRO] falta o mc (cliente do MinIO): veja docs/SERVIDOR.md" >&2
    exit 1
fi

REPO=$(mktemp -d)
trap 'rm -rf "$REPO"' EXIT

# Traz o que já está publicado: o `--multiversion` abaixo mantém as versões antigas na
# lista, para quem quiser fixar uma, e para isso elas precisam estar no índice novo.
mc mirror --quiet "$BUCKET" "$REPO" || true
cp -f "$DEB" "$REPO/"
cd "$REPO"

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
# primeiro. O `Release.gpg` fica para clientes antigos.
gpg --batch --yes --clearsign --local-user "$SIGNER" -o InRelease Release
gpg --batch --yes --detach-sign --armor --local-user "$SIGNER" -o Release.gpg Release
gpg --export "$SIGNER" > unkvoid.gpg

# O .deb primeiro e o índice por último: um `apt update` no meio do caminho encontra um
# índice antigo apontando para arquivos que já existem, nunca o contrário.
mc cp --quiet ./*.deb "$BUCKET/"
mc cp --quiet unkvoid.gpg Packages Packages.gz Release Release.gpg InRelease "$BUCKET/"

echo "[INFO] repositório atualizado em $BUCKET"
mc ls "$BUCKET" | grep '\.deb$' | sed 's/.* //'
