#!/usr/bin/env bash
#
# Registra um runner do GitHub Actions para o repositório unkvoid, como serviço, nesta
# VPS. Cada repositório tem o próprio runner numa pasta própria — os de tarkas e retro
# já existem e não são tocados.
#
# O token de registro vale uma hora e sai daqui, no notebook:
#   gh api -X POST repos/edsuuu/unkvoid/actions/runners/registration-token --jq .token
#
#   ./runner-install.sh <token>
set -euo pipefail

TOKEN="${1:?token de registro do GitHub}"
DIR="$HOME/actions-runner-unkvoid"

if [ -f "$DIR/.runner" ]; then
    echo "[INFO] runner já registrado em $DIR"
    exit 0
fi

mkdir -p "$DIR"
cd "$DIR"

VERSION=$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest | node -pe 'JSON.parse(require("fs").readFileSync(0)).tag_name.slice(1)')
curl -fsSL -o runner.tar.gz "https://github.com/actions/runner/releases/download/v${VERSION}/actions-runner-linux-x64-${VERSION}.tar.gz"
tar xzf runner.tar.gz && rm runner.tar.gz

./config.sh --unattended --replace \
    --url https://github.com/edsuuu/unkvoid \
    --token "$TOKEN" \
    --name unkvoid-vps \
    --labels unkvoid \
    --work _work

sudo ./svc.sh install "$(id -un)"
sudo ./svc.sh start
sudo ./svc.sh status
