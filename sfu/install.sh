#!/usr/bin/env bash
#
# A metade do deploy do SFU que roda NA VPS, com o `dist/` já no disco: instala as
# dependências de produção, espera a sala esvaziar e reinicia pelo pm2.
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

# Reiniciar derruba toda sala que estiver no ar: os workers do mediasoup morrem junto
# com o processo. O app se recupera sozinho (reconecta, republica), mas custa alguns
# segundos de tela preta para todo mundo. Então espera esvaziar primeiro, e só passa por
# cima do limite se a espera estourar.
#
# ponytail: espera-esvaziar, não blue/green. Zero downtime de verdade exigiria dois
# processos em faixas de porta UDP diferentes e o firewall do painel aberto para as duas;
# vale o custo só quando houver gente na sala a qualquer hora do dia.
DRAIN_MINUTES="${SFU_DRAIN_MINUTES:-30}"
DEADLINE=$(( $(date +%s) + DRAIN_MINUTES * 60 ))

while true; do
    HEALTH=$(curl -sf http://127.0.0.1:3000/health || echo '{"rooms":0,"peers":0}')
    ROOMS=$(node -pe 'JSON.parse(process.argv[1]).rooms' "$HEALTH")
    PEERS=$(node -pe 'JSON.parse(process.argv[1]).peers' "$HEALTH")

    if [ "$ROOMS" = "0" ] && [ "$PEERS" = "0" ]; then
        break
    fi

    if [ "$(date +%s)" -ge "$DEADLINE" ]; then
        echo "[WARN] $ROOMS sala(s) e $PEERS pessoa(s) ainda no ar depois de $DRAIN_MINUTES min — reiniciando assim mesmo"
        break
    fi

    echo "[INFO] $ROOMS sala(s), $PEERS pessoa(s) no ar — esperando esvaziar"
    sleep 15
done

pm2 startOrRestart ecosystem.config.cjs --update-env
pm2 save
sleep 2
curl -sf http://127.0.0.1:3000/health && echo
