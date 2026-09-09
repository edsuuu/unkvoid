#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

REMOTE="${1:-vps}"
TARGET="/var/www/projects/sfu"

echo "[INFO] compilando TypeScript"
pnpm run build

echo "[INFO] uploading to $REMOTE:$TARGET"
rsync -az --exclude node_modules \
    ./dist ./check.mjs ./package.json ./pnpm-lock.yaml ./pnpm-workspace.yaml ./ecosystem.config.cjs \
    "$REMOTE:$TARGET/"

# Note: pnpm 11 ignores onlyBuiltDependencies and still EXITS WITH AN ERROR for this,
# so installation is allowed to fail and the binary check decides whether it succeeded.
# Guard: fails if mediasoup changes the script name. Upgrade: use npm instead of pnpm.
ssh "$REMOTE" "set -e
    cd $TARGET
    pnpm install --prod --frozen-lockfile || true
    WORKER=node_modules/mediasoup/worker/out/Release/mediasoup-worker
    if [ ! -x \"\$WORKER\" ]; then
        (cd node_modules/mediasoup && node npm-scripts.mjs postinstall)
    fi
    if [ ! -x \"\$WORKER\" ]; then
        echo '[ERROR] mediasoup worker was not installed' >&2
        exit 1
    fi
    pm2 delete sfu > /dev/null 2>&1 || true
    pm2 start ecosystem.config.cjs --update-env
    pm2 save
    sleep 2
    curl -sf http://127.0.0.1:3000/health && echo"

echo "[INFO] SFU is running"
