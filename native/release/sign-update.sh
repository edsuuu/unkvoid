#!/usr/bin/env bash
#
# Assina um instalador no formato que o atualizador já entende: o texto minisign inteiro,
# codificado em Base64. O encapsulamento evita mudar o contrato de `latest.json`.
#
#   UNKVOID_UPDATE_SECRET_KEY=~/auxilos/unkvoid.key ./sign-update.sh instalador.exe
set -euo pipefail

installer="${1:?informe o instalador}"
signature="${2:-$installer.sig}"
secret_key="${UNKVOID_UPDATE_SECRET_KEY:?UNKVOID_UPDATE_SECRET_KEY deve apontar para a chave privada}"
script_directory=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
temporary_signature=$(mktemp)
temporary_secret_key=""

cleanup() {
    rm -f "$temporary_signature"

    if [ -n "$temporary_secret_key" ]; then
        rm -f "$temporary_secret_key"
    fi
}

trap cleanup EXIT

if ! command -v minisign > /dev/null 2>&1; then
    echo "[ERRO] falta o minisign; veja docs/AUTO-UPDATE.md" >&2
    exit 1
fi

if [ ! -f "$secret_key" ]; then
    echo "[ERRO] a chave privada $secret_key não existe" >&2
    exit 1
fi

minisign_secret_key="$secret_key"
IFS= read -r first_line < "$secret_key" || true

if [[ "$first_line" != "untrusted comment:"* ]]; then
    # A chave herdada do Tauri encapsula o arquivo minisign em Base64; decodificamos apenas em arquivo privado.
    temporary_secret_key=$(mktemp)
    chmod 600 "$temporary_secret_key"

    if ! base64 --decode < "$secret_key" > "$temporary_secret_key" 2> /dev/null \
        && ! base64 -D < "$secret_key" > "$temporary_secret_key" 2> /dev/null; then
        echo "[ERRO] a chave privada deve ser um arquivo minisign ou seu conteúdo codificado em Base64" >&2
        exit 1
    fi

    minisign_secret_key="$temporary_secret_key"
fi

minisign -S -s "$minisign_secret_key" -m "$installer" -x "$temporary_signature"
base64 < "$temporary_signature" | tr -d '\r\n' > "$signature"

# Conferir aqui evita publicar uma chave privada que o aplicativo já não reconhece.
node "$script_directory/check-signature.mjs" "$signature"
