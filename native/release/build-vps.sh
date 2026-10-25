#!/usr/bin/env bash
#
# Build do Linux na VPS e publicação no repositório APT.
#
# A VPS gera só o `.deb`, dentro do Debian 12 do app único. Uma folga de CPU mantém o
# SFU responsivo enquanto o Rust compila; publicar atualização nunca pode travar uma chamada.
#
#   native/release/build-vps.sh
#   make build-vps
set -euo pipefail

SCRIPT_DIRECTORY=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
NATIVE_DIRECTORY=$(cd "$SCRIPT_DIRECTORY/.." && pwd)

"$NATIVE_DIRECTORY/apps/unkvoid/build-deb.sh"

# A pasta preserva versões anteriores; a mais nova por data é a que acabou de ser compilada.
PACKAGE_FILE=$(find "$NATIVE_DIRECTORY/target/deb12" -maxdepth 1 -name '*.deb' -printf '%T@ %p\n' 2>/dev/null \
    | sort -rn | head -1 | cut -d' ' -f2- || true)

if [ -z "$PACKAGE_FILE" ]; then
    echo "[ERRO] nenhum .deb saiu do build" >&2
    exit 1
fi

for argument in "$@"; do
    if [ "$argument" = "--dry-run" ]; then
        echo "[INFO] --dry-run: nada publicado: $PACKAGE_FILE"
        exit 0
    fi
done

"$SCRIPT_DIRECTORY/apt-publish.sh" "$PACKAGE_FILE"

# O card do Linux no site aponta para o `.deb` mais recente. O segredo fica só na VPS.
web_environment="${UNKVOID_WEB_ENV:-/var/www/projects/unkvoid-web/shared/.env}"

if [ -f "$web_environment" ]; then
    RELEASE_SECRET="$(grep '^RELEASE_SECRET=' "$web_environment" | cut -d= -f2-)" \
        "$SCRIPT_DIRECTORY/publish-release.sh" linux-x86_64-deb "$PACKAGE_FILE"
fi

# O APT é o canal de atualização no Linux. O GitHub recebe uma cópia somente quando for pedido.
case " $* " in
    *" --github "*) ;;
    *)
        echo "[INFO] pronto: .deb no APT. Para publicar também no GitHub: $0 --github"
        exit 0
        ;;
esac

if ! command -v gh > /dev/null 2>&1; then
    echo "[ERRO] falta o gh na VPS." >&2
    exit 1
fi

if ! command -v node > /dev/null 2>&1; then
    echo "[ERRO] falta o node na VPS para publicar a release no GitHub." >&2
    exit 1
fi

if ! gh auth status > /dev/null 2>&1; then
    echo "[ERRO] o gh não está autenticado nesta VPS. Rode uma vez: gh auth login" >&2
    exit 1
fi

node "$SCRIPT_DIRECTORY/release.mjs" "$@"
