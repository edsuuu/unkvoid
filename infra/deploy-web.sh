#!/usr/bin/env bash
set -euo pipefail

REMOTE="${1:-vps}"
TARGET="/var/www/projects/discord"
APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../web" && pwd)"

echo "[INFO] build dos assets"
cd "$APP_DIR"
pnpm install --frozen-lockfile
pnpm run build

echo "[INFO] enviando para $REMOTE:$TARGET"
ssh "$REMOTE" "sudo mkdir -p $TARGET/{current,shared/storage} && sudo chown -R \$(id -un):\$(id -gn) $TARGET"

# .env e storage moram no shared e sobrevivem ao deploy
rsync -az --delete \
    --exclude node_modules --exclude .git --exclude .env --exclude .env.testing \
    --exclude storage --exclude vendor --exclude .idea \
    ./ "$REMOTE:$TARGET/current/"

ssh "$REMOTE" "set -e
    cd $TARGET/current
    rm -rf storage && ln -sfn $TARGET/shared/storage storage
    composer install --no-dev --optimize-autoloader --no-interaction --quiet
    ln -sfn $TARGET/shared/.env .env
    php artisan migrate --force
    php artisan optimize
    ln -sfn $TARGET/shared/storage/app/public public/storage
    sudo chown -R \$(id -un):www-data $TARGET
    sudo chmod -R 2775 $TARGET/shared/storage

    # Reverb carrega a configuração no boot: sem reiniciar, ele continua com a chave e
    # a porta do deploy anterior e o chat para de autenticar.
    pm2 delete reverb > /dev/null 2>&1 || true
    pm2 start reverb.config.cjs --update-env
    pm2 save"

echo "[INFO] pronto: https://discord.unkvoid.com"
