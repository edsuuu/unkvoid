#!/usr/bin/env bash
#
# Deploy do site (web/) na VPS, sem derrubar ninguém: cada versão vai para uma pasta
# própria, e o `current` só troca de alvo quando tudo está pronto. O php-fpm recarrega
# em seguida para esquecer o caminho antigo.
#
# Roda NA VPS, pelo runner do GitHub Actions, a partir do checkout do repositório:
#   infra/deploy-web.sh              (usa o web/ ao lado deste script)
#   infra/deploy-web.sh /outro/web   (ou um caminho explícito)
#
# Espelha o deploy.template.sh do Linux-Devlopment; a diferença é que o código já
# está no disco, então não há clone.
set -euo pipefail

SOURCE="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../web" && pwd)}"
PROJECT_DIR="${UNKVOID_WEB_DIR:-/var/www/projects/unkvoid-web}"
KEEP_RELEASES=3
PHP="${PHP_BINARY:-/usr/bin/php8.4}"

if [ ! -d "$PROJECT_DIR/releases" ] || [ ! -f "$PROJECT_DIR/shared/.env" ]; then
    echo "[ERRO] falta a estrutura em $PROJECT_DIR (releases/, shared/.env) — veja SERVIDOR.md" >&2
    exit 1
fi

RELEASE="$(date +%Y-%m-%d-%H%M%S)"
RELEASE_DIR="$PROJECT_DIR/releases/$RELEASE"

echo "[INFO] copiando $SOURCE para $RELEASE_DIR"
rsync -a --exclude node_modules --exclude vendor --exclude .env --exclude storage --exclude public/build \
    "$SOURCE/" "$RELEASE_DIR/"
cd "$RELEASE_DIR"

mkdir -p bootstrap/cache
ln -s "$PROJECT_DIR/shared/storage" storage
cp "$PROJECT_DIR/shared/.env" .env
chmod 600 .env
ln -sfn "$PROJECT_DIR/shared/storage/app/public" public/storage

composer install --no-dev --optimize-autoloader --no-interaction --prefer-dist --no-progress

pnpm install --frozen-lockfile
pnpm run build
rm -rf node_modules

sudo chgrp -R www-data bootstrap/cache
sudo chmod -R 2775 bootstrap/cache

"$PHP" artisan migrate --force
"$PHP" artisan db:seed --class=Seeder001Roles --force
"$PHP" artisan optimize

ln -sfn "$RELEASE_DIR" "$PROJECT_DIR/current"
echo "[INFO] release $RELEASE ativa em $PROJECT_DIR/current"

sudo systemctl reload php8.4-fpm

# Pela data de modificação, e não pelo nome.
#
# O nome é um carimbo da hora local. No dia em que o fuso da máquina saiu de CEST para o
# de São Paulo, o relógio andou cinco horas para trás e a release recém-criada virou a
# "mais antiga" da lista ordenada por nome. A limpeza apagou justamente aquela para onde
# o `current` tinha acabado de apontar, e o site foi para 404 com o deploy marcado como
# sucesso. A data de modificação não depende de como a máquina resolveu chamar a hora.
#
# A conferência do `current` fica como segunda trava: seja qual for a ordem, a release
# que está no ar não sai do disco.
cd "$PROJECT_DIR/releases"
CURRENT=$(basename "$(readlink -f "$PROJECT_DIR/current")")

ls -1t | tail -n +$((KEEP_RELEASES + 1)) | while read -r OLD_RELEASE; do
    if [ "$OLD_RELEASE" = "$CURRENT" ]; then
        continue
    fi

    echo "[INFO] removendo release antiga $OLD_RELEASE"
    rm -rf "$OLD_RELEASE"
done

curl -sf -o /dev/null -w "[INFO] site respondeu %{http_code}\n" http://127.0.0.1/up -H 'Host: unkvoid.com'
