#!/usr/bin/env bash
set -euo pipefail

REMOTE="${1:-vps}"
TARGET="/var/www/projects/sfu"

echo "[INFO] enviando para $REMOTE:$TARGET"
rsync -az --exclude node_modules \
    ./src ./check.mjs ./.env.example ./package.json ./pnpm-lock.yaml ./pnpm-workspace.yaml ./ecosystem.config.cjs \
    "$REMOTE:$TARGET/"

# ponytail: o pnpm 11 ignora onlyBuiltDependencies e ainda SAI COM ERRO por isso,
# então o install roda tolerante e a checagem do binário é que decide se deu certo.
# Teto: quebra se o mediasoup mudar o nome do script. Upgrade: npm no lugar do pnpm.
ssh "$REMOTE" "set -e
    cd $TARGET
    pnpm install --prod --frozen-lockfile || true
    WORKER=node_modules/mediasoup/worker/out/Release/mediasoup-worker
    if [ ! -x \"\$WORKER\" ]; then
        (cd node_modules/mediasoup && node npm-scripts.mjs postinstall)
    fi
    if [ ! -x \"\$WORKER\" ]; then
        echo '[ERRO] worker do mediasoup nao foi instalado' >&2
        exit 1
    fi
    pm2 startOrRestart ecosystem.config.cjs --update-env
    pm2 save
    sleep 2
    curl -sf http://127.0.0.1:3000/health && echo"

echo "[INFO] SFU no ar"
