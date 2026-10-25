#!/usr/bin/env bash
#
# Registra um instalador no site, que o guarda no MinIO e o serve por URL assinada.
#
#   RELEASE_SECRET=... ./publish-release.sh <plataforma> <arquivo> [<arquivo.sig>]
#   RELEASE_VERSION=0.0.7 ...  para registrar um build de outra versão
#
# A plataforma é a chave que o atualizador procura: windows-x86_64-nsis,
# darwin-aarch64, darwin-aarch64-dmg ou linux-x86_64-deb. A versão sai do único
# `workspace.package` do Cargo, para instalador e aplicativo não divergirem.
#
# A chamada é assinada com HMAC sobre hora, método, caminho e o SHA-256 do arquivo: sem
# conta, sem token de sessão, e um arquivo trocado no caminho invalida a assinatura.
set -euo pipefail

PLATFORM="${1:?plataforma}"
INSTALLER_FILE="${2:?arquivo}"
SIGNATURE_FILE="${3:-}"
RELEASE_SECRET_VALUE="${RELEASE_SECRET:?RELEASE_SECRET ausente no ambiente}"
SITE_URL="${UNKVOID_SITE:-https://unkvoid.com}"
NATIVE_DIRECTORY=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

# RELEASE_VERSION serve para registrar um build antigo que ficou para trás.
VERSION="${RELEASE_VERSION:-$(awk '
    /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^version[[:space:]]*=/ {
        value = $0
        sub(/^[^"]*"/, "", value)
        sub(/".*$/, "", value)
        print value
        exit
    }
' "$NATIVE_DIRECTORY/Cargo.toml")}"

if [ -z "$VERSION" ]; then
    echo "[ERRO] não encontrei workspace.package.version em $NATIVE_DIRECTORY/Cargo.toml" >&2
    exit 1
fi
TIMESTAMP=$(date +%s)
FILE_HASH=$(sha256sum "$INSTALLER_FILE" | cut -d' ' -f1)
REQUEST_SIGNATURE=$(printf '%s\n%s\n%s\n%s' "$TIMESTAMP" POST /api/releases "$FILE_HASH" | openssl dgst -sha256 -hmac "$RELEASE_SECRET_VALUE" | awk '{print $NF}')

SIGNATURE_ARGUMENTS=()

if [ -n "$SIGNATURE_FILE" ]; then
    if [ ! -f "$SIGNATURE_FILE" ]; then
        echo "[ERRO] assinatura $SIGNATURE_FILE não existe" >&2
        exit 1
    fi

    SIGNATURE_ARGUMENTS=(-F "signature=<$SIGNATURE_FILE")
fi

curl -sfS -X POST "$SITE_URL/api/releases" \
    -H 'Accept: application/json' \
    -H "X-Unkvoid-Timestamp: $TIMESTAMP" \
    -H "X-Unkvoid-Signature: $REQUEST_SIGNATURE" \
    -F "version=$VERSION" \
    -F "platform=$PLATFORM" \
    -F "file=@$INSTALLER_FILE" \
    "${SIGNATURE_ARGUMENTS[@]}"
echo
echo "[INFO] $PLATFORM $VERSION publicado em $SITE_URL"
