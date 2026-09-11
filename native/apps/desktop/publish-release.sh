#!/usr/bin/env bash
#
# Registra um instalador no site, que o guarda no MinIO e o serve por URL assinada.
#
#   RELEASE_SECRET=... ./publish-release.sh <plataforma> <arquivo> [<arquivo.sig>]
#   RELEASE_VERSION=0.0.7 ...  para registrar um build de outra versão
#
# A plataforma é a chave que o atualizador do Tauri procura: windows-x86_64-msi,
# windows-x86_64-nsis, darwin-aarch64 (o .app.tar.gz), darwin-aarch64-dmg ou
# linux-x86_64-deb. A versão sai do tauri.conf.json ao lado.
#
# A chamada é assinada com HMAC sobre hora, método, caminho e o SHA-256 do arquivo: sem
# conta, sem token de sessão, e um arquivo trocado no caminho invalida a assinatura.
set -euo pipefail

PLATFORM="${1:?plataforma}"
FILE="${2:?arquivo}"
SIG="${3:-}"
SECRET="${RELEASE_SECRET:?RELEASE_SECRET ausente no ambiente}"
SITE="${UNKVOID_SITE:-https://unkvoid.com}"

# RELEASE_VERSION serve para registrar um build antigo que ficou para trás.
VERSION="${RELEASE_VERSION:-$(node -pe 'JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).version' "$(dirname "${BASH_SOURCE[0]}")/src-tauri/tauri.conf.json")}"
TIMESTAMP=$(date +%s)
HASH=$(sha256sum "$FILE" | cut -d' ' -f1)
SIGNATURE=$(printf '%s\n%s\n%s\n%s' "$TIMESTAMP" POST /api/releases "$HASH" | openssl dgst -sha256 -hmac "$SECRET" | awk '{print $NF}')

if [ -n "$SIG" ] && [ ! -f "$SIG" ]; then
    echo "[ERRO] assinatura $SIG não existe" >&2
    exit 1
fi

curl -sfS -X POST "$SITE/api/releases" \
    -H 'Accept: application/json' \
    -H "X-Unkvoid-Timestamp: $TIMESTAMP" \
    -H "X-Unkvoid-Signature: $SIGNATURE" \
    -F "version=$VERSION" \
    -F "platform=$PLATFORM" \
    -F "file=@$FILE" \
    ${SIG:+-F "signature=<$SIG"}
echo
echo "[INFO] $PLATFORM $VERSION publicado em $SITE"
