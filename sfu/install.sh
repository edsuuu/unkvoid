#!/usr/bin/env bash
#
# A metade do deploy do SFU que roda NA VPS, com o `dist/` já no disco: instala as
# dependências de produção e reinicia pelo pm2.
#
# Reiniciar derruba toda sala no ar: os workers do mediasoup morrem com o processo, e
# quem estava em chamada leva alguns segundos de tela preta até o app reconectar e
# republicar sozinho. É uma escolha: o deploy sai na hora em vez de esperar esvaziar.
#
# Chamado pelo deploy.sh (via ssh, do notebook) e pelo runner do GitHub Actions.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# O pnpm 11 ignora o onlyBuiltDependencies e ainda assim SAI COM ERRO por causa disso,
# então a instalação pode falhar e quem decide é a existência do binário do worker.
pnpm install --prod --frozen-lockfile || true

WORKER=node_modules/mediasoup/worker/out/Release/mediasoup-worker

if [ ! -x "$WORKER" ]; then
    (cd node_modules/mediasoup && node npm-scripts.mjs postinstall)
fi

if [ ! -x "$WORKER" ]; then
    echo "[ERRO] o worker do mediasoup não foi instalado" >&2
    exit 1
fi

# `--only sfu` para o pm2 não mexer em nada mais que venha a existir neste arquivo.
pm2 startOrRestart ecosystem.config.cjs --only sfu --update-env
pm2 save
sleep 2
curl -sf http://127.0.0.1:3000/health && echo
