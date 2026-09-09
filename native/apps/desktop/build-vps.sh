#!/usr/bin/env bash
#
# Build do Linux na VPS, assinado, e publicado na release.
#
# A VPS é Linux, então ela gera .deb e .AppImage e mais nada: o .dmg exige um Mac por
# licença da Apple, e o .msi exige o WiX rodando no Windows. Essas duas plataformas vêm
# do GitHub Actions (.github/workflows/release.yml), e o `release.mjs` mescla o
# manifesto publicado em vez de sobrescrevê-lo — é isso que deixa as três conviverem na
# mesma release sem uma apagar a outra.
#
# Roda NA VPS:
#   TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/unkvoid.key)" ./build-vps.sh
#
# Ou daqui, pelo alias `vps` do ssh:
#   make build-vps
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

# Sem a chave o build sai sem `.sig`, a release sobe igual e ninguém se atualiza
# sozinho para ela. Falhar agora é melhor do que descobrir isso na máquina de alguém.
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    echo "[ERRO] TAURI_SIGNING_PRIVATE_KEY não está definida — o build sairia sem assinatura." >&2
    echo "       TAURI_SIGNING_PRIVATE_KEY=\"\$(cat ~/.tauri/unkvoid.key)\" $0" >&2
    exit 1
fi

export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

# O que o Tauri precisa para empacotar no Ubuntu, mais o cmake, que o `opusic-sys` usa
# para compilar o libopus do zero.
#
# A verificação é pacote a pacote de propósito. Guardar a lista inteira atrás de um
# `dpkg -s` de um único pacote fazia a segunda execução pular tudo, e foi assim que a
# falta do cmake só apareceu depois de quarenta minutos compilando.
PACKAGES="build-essential curl wget file pkg-config cmake
libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev
librsvg2-dev libxdo-dev"

MISSING=""

for package in $PACKAGES; do
    dpkg -s "$package" > /dev/null 2>&1 || MISSING="$MISSING $package"
done

if [ -n "$MISSING" ]; then
    echo "[INFO] instalando:$MISSING"
    sudo apt-get update
    # shellcheck disable=SC2086
    sudo apt-get install -y $MISSING
fi

if ! command -v cargo > /dev/null 2>&1; then
    echo "[INFO] instalando o Rust"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
fi

# shellcheck disable=SC1090
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# Esta máquina também é o SFU, e mediasoup é tempo real: um build ocupando todos os
# núcleos vira engasgo na tela de quem está assistindo agora. Uma thread de folga e
# prioridade baixa custam alguns minutos a mais e não custam a chamada de ninguém.
CORES=$(nproc)
export CARGO_BUILD_JOBS=$(( CORES > 1 ? CORES - 1 : 1 ))

echo "[INFO] compilando com $CARGO_BUILD_JOBS de $CORES núcleos, em prioridade baixa"

npm ci
npm run check
# `UNKVOID_BUNDLES=deb` pula o AppImage, que baixa tool própria e leva alguns
# minutos a mais — útil quando se quer só um instalador para testar.
BUNDLES="${UNKVOID_BUNDLES:-deb,appimage}"

nice -n 19 npx tauri build --bundles "$BUNDLES"

# Os instaladores ficam servidos pelo nginx em /downloads/, para quem for testar não
# precisar de ssh nem de esperar a release sair.
DOWNLOADS="${UNKVOID_DOWNLOADS:-/var/www/downloads/unkvoid}"

if [ -d "$DOWNLOADS" ]; then
    find ../../target/release/bundle -maxdepth 2 -type f \
        \( -name '*.deb' -o -name '*.AppImage' -o -name '*.sig' \) \
        -exec cp -f {} "$DOWNLOADS/" \;

    echo "[INFO] instaladores em $DOWNLOADS"
fi

# Com --dry-run o build para aqui: serve para gerar um instalador de teste sem mexer na
# release, e sem exigir um gh autenticado nesta máquina.
for arg in "$@"; do
    if [ "$argumento" = "--dry-run" ]; then
        echo "[INFO] --dry-run: instaladores gerados, nada publicado"
        find ../../target/release/bundle -maxdepth 2 -type f \( -name '*.deb' -o -name '*.AppImage' -o -name '*.sig' \) -print
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
    if ! command -v "$ferramenta" > /dev/null 2>&1; then
        echo "[ERRO] falta o $ferramenta na VPS." >&2
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
