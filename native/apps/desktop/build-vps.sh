#!/usr/bin/env bash
#
# Build do Linux na VPS e publicação no repositório APT.
#
# A VPS é Linux, então ela gera o .deb e mais nada: o .dmg exige um Mac por licença da
# Apple, e o .msi exige o WiX rodando no Windows.
#
# **No Linux quem atualiza é o APT**, e é por isso que a chave do auto-update não
# aparece aqui. O atualizador embutido do Tauri está desligado neste sistema — pedir
# senha de root com `pkexec` no meio da abertura faria o que o `apt upgrade` já faz
# junto com o resto da máquina. Quem autentica o pacote é a assinatura GPG do próprio
# repositório, que é outra chave e mora só nesta VPS.
#
# Isso também é o que mantém a chave que assina atualizações FORA de uma máquina
# exposta à internet: quem a tiver publica atualização para todo mundo que instalou o
# app, e aqui ela não serviria para nada.
#
# Roda NA VPS:
#   ./build-vps.sh
#
# Ou daqui, pelo alias `vps` do ssh:
#   make build-vps
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# O .deb é compilado DENTRO de um Debian 12 (Dockerfile.linux): um binário fica preso à
# glibc da máquina que o gera, e a VPS, Ubuntu 24.04, produzia um app que não abria em
# Debian 12 nem em Parrot. A distro mais velha que queremos suportar é a que compila.
#
# Esta máquina também é o SFU, e mediasoup é tempo real: um build ocupando todos os
# núcleos vira engasgo na tela de quem está assistindo agora. Uma thread de folga e
# prioridade baixa custam alguns minutos a mais e não custam a chamada de ninguém.
CORES=$(nproc)
JOBS=$(( CORES > 1 ? CORES - 1 : 1 ))
IMAGE=unkvoid-linux-builder
NATIVE=$(cd ../.. && pwd)

# Só o .deb: é o que o APT distribui e o que o app espera no Linux. `UNKVOID_BUNDLES`
# ainda aceita `deb,appimage` para quem precisar do AppImage solto.
BUNDLES="${UNKVOID_BUNDLES:-deb}"

# As conferências olham o repositório inteiro (quatro níveis acima), e dentro do
# container só existe o `native/`. Rodam aqui fora, onde o host tem Node e Python.
npm ci
npm run check

echo "[INFO] imagem de build (Debian 12)"
docker build -q -t "$IMAGE" -f Dockerfile.linux . > /dev/null

echo "[INFO] compilando com $JOBS de $CORES núcleos, em prioridade baixa, dentro do Debian 12"

# Cache do cargo e do npm ficam em pastas do próprio checkout, com o uid de quem chama:
# o container não deixa nada de root para trás. `target-deb12` é separado do `target`
# do host de propósito — são objetos de outra glibc.
#
# `createUpdaterArtifacts: false` só para este build. Com a chave pública no
# `tauri.conf.json`, o Tauri tenta assinar o artefato de updater de TODO bundle e para o
# build inteiro quando não acha a privada — mesmo gerando um `.deb`, que não tem updater.
docker run --rm --user "$(id -u):$(id -g)" \
    -v "$NATIVE:/work" -w /work/apps/desktop \
    -e HOME=/work/.home -e CARGO_HOME=/work/.cargo-home -e CARGO_TARGET_DIR=/work/target-deb12 \
    -e CARGO_BUILD_JOBS="$JOBS" -e npm_config_cache=/work/.home/.npm \
    "$IMAGE" bash -c "mkdir -p /work/.home && nice -n 19 npx tauri build --bundles $BUNDLES --config '{\"bundle\":{\"createUpdaterArtifacts\":false}}'"

# O repositório APT: é por ele que o Linux instala e atualiza, com `apt install unkvoid`.
# O download solto continua existindo para quem só quer o arquivo.
# O mais recente, não o primeiro que a busca achar: a pasta guarda os `.deb` de todas
# as versões já geradas nesta máquina, e `-print -quit` publicava um antigo no APT.
DEB=$(find ../../target-deb12/release/bundle/deb -maxdepth 1 -name '*.deb' -printf '%T@ %p\n' 2>/dev/null \
    | sort -rn | head -1 | cut -d' ' -f2- || true)

if [ -n "$DEB" ]; then
    ./apt-publish.sh "$DEB"
fi

# O card do Linux no site aponta para o .deb mais novo. O segredo mora no .env do site,
# que está nesta mesma máquina, então nada viaja.
WEB_ENV="${UNKVOID_WEB_ENV:-/var/www/projects/unkvoid-web/shared/.env}"

if [ -n "$DEB" ] && [ -f "$WEB_ENV" ]; then
    RELEASE_SECRET="$(grep '^RELEASE_SECRET=' "$WEB_ENV" | cut -d= -f2-)" ./publish-release.sh linux-x86_64-deb "$DEB"
fi

# Daqui para baixo é só o GitHub, e ninguém se atualiza por ele: o macOS e o Windows
# leem o manifesto do nosso próprio servidor, e o Linux lê o APT logo acima. A release
# lá é arquivo para quem quiser baixar à mão, então só acontece se for pedida — sem
# isto, um `gh` não autenticado transformava um build já publicado no APT em erro.
case " $* " in
    *" --github "*) ;;
    *)
        echo "[INFO] pronto: .deb no APT. Para publicar também no GitHub: $0 --github"
        exit 0
        ;;
esac

# Com --dry-run o build para aqui: serve para gerar um instalador de teste sem mexer na
# release, e sem exigir um gh autenticado nesta máquina.
for arg in "$@"; do
    if [ "$arg" = "--dry-run" ]; then
        echo "[INFO] --dry-run: instaladores gerados, nada publicado"
        find ../../target-deb12/release/bundle -maxdepth 2 -type f \( -name '*.deb' -o -name '*.AppImage' -o -name '*.sig' \) -print
        exit 0
    fi
done

# O `gh` é quem publica a release. Não vem no Ubuntu, então instala do repositório da
# própria GitHub — a versão do apt padrão é velha demais para `release upload --clobber`.
if ! command -v gh > /dev/null 2>&1; then
    echo "[INFO] instalando o gh"
    sudo mkdir -p -m 755 /etc/apt/keyrings
    curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
        | sudo tee /etc/apt/keyrings/githubcli-archive-keyring.gpg > /dev/null
    sudo chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
        | sudo tee /etc/apt/sources.list.d/github-cli.list > /dev/null
    sudo apt-get update
    sudo apt-get install -y gh
fi

for tool in node npm gh; do
    if ! command -v "$tool" > /dev/null 2>&1; then
        echo "[ERRO] falta o $tool na VPS." >&2
        exit 1
    fi
done

# Um passo manual, uma vez só. Mandar o token daqui a cada build significaria um segredo
# a mais viajando por ssh para nada: o `gh` guarda o dele e renova sozinho.
if ! gh auth status > /dev/null 2>&1; then
    echo "[ERRO] o gh não está autenticado nesta VPS." >&2
    echo "       Rode uma vez, aqui dentro:  gh auth login" >&2
    exit 1
fi


# Publica só o que esta máquina gerou. O macOS e o Windows entram na mesma release por
# outro caminho, e o manifesto é mesclado, não substituído.
node release.mjs "$@"
