#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

REMOTE="${1:-vps}"
TARGET="/var/www/projects/sfu"

echo "[INFO] compilando TypeScript"
pnpm run build

echo "[INFO] uploading to $REMOTE:$TARGET"
rsync -az --exclude node_modules \
    ./dist ./check.mjs ./check-heartbeat.mjs ./install.sh ./.env.example ./package.json ./pnpm-lock.yaml ./pnpm-workspace.yaml ./ecosystem.config.cjs \
    "$REMOTE:$TARGET/"

# Daqui em diante é o install.sh, que é o mesmo que o runner do GitHub Actions roda: ele
# espera a sala esvaziar antes de reiniciar, para não derrubar quem está no ar.
ssh "$REMOTE" "cd $TARGET && ./install.sh"

echo "[INFO] SFU is running"
