#!/usr/bin/env bash
#
# Publica um instalador no servidor e costura o manifesto de atualização.
#
# Existe para o auto-update não depender do GitHub. Cada sistema é compilado numa máquina
# diferente — Windows no Windows, macOS num Mac, Linux na VPS — e as três chamam este
# script apontando para o mesmo `latest.json`. Por isso ele COSTURA em vez de escrever:
# publicar o Windows não pode apagar o macOS que subiu ontem.
#
#   ./publish-downloads.sh 0.0.7 windows-x86_64 Unkvoid_0.0.7_x64_pt-BR.msi
#
# A assinatura é lida do arquivo `.sig` ao lado. Sem ela o instalador sobe mas fica de
# fora do manifesto: quem se atualiza sozinho exige assinatura, e um manifesto apontando
# para um arquivo não assinado é uma atualização que ninguém consegue instalar.
set -euo pipefail

VERSION="${1:?uso: publish-downloads.sh <versao> <plataforma> <arquivo>}"
PLATFORM="${2:?plataforma, por exemplo linux-x86_64, windows-x86_64, darwin-aarch64}"
FILE="${3:?caminho do instalador}"

REMOTE="${UNKVOID_REMOTE:-vps}"
DIR="${UNKVOID_DOWNLOADS:-/var/www/downloads/unkvoid}"
BASE="${UNKVOID_BASE_URL:-https://discord.unkvoid.com/downloads}"

[ -f "$FILE" ] || { echo "[ERRO] não achei $FILE" >&2; exit 1; }
[ -f "$FILE.sig" ] || { echo "[ERRO] falta $FILE.sig — build sem assinatura não atualiza ninguém" >&2; exit 1; }

NAME=$(basename "$FILE")
SIGNATURE=$(cat "$FILE.sig")

# Rodando NA VPS não há o que copiar nem por onde passar: o `ssh` para si mesmo exigiria
# uma chave que a máquina não precisa ter.
if [ -d "$DIR" ]; then
    run() { bash -c "$1"; }
    cp -f "$FILE" "$FILE.sig" "$DIR/"
else
    run() { ssh "$REMOTE" "$1"; }
    scp -q "$FILE" "$FILE.sig" "$REMOTE:$DIR/"
fi

echo "[INFO] $NAME publicado em $BASE/"

# O merge em Node porque a VPS já tem Node — é o mesmo runtime do SFU, e `jq` seria um
# pacote a mais para instalar em toda máquina que publica.
# A assinatura vai pelo ambiente, não interpolada no código: `${SIGNATURE@Q}` só existe
# do bash 4.4 em diante, e o macOS ainda vem com o 3.2 — justamente a máquina que gera o
# artefato do macOS. O script morria com "bad substitution" só ali.
run "cd '$DIR' && UNKVOID_SIGNATURE='$SIGNATURE' node -e '
const fs = require(\"fs\");
const path = \"latest.json\";
const manifest = fs.existsSync(path)
    ? JSON.parse(fs.readFileSync(path, \"utf8\"))
    : { version: \"$VERSION\", notes: \"\", platforms: {} };

// A versão do manifesto é a mais nova que já passou por aqui. Publicar uma correção só
// para macOS não pode rebaixar a versão que o Windows já anuncia.
if (manifest.version.localeCompare(\"$VERSION\", undefined, { numeric: true }) < 0) {
    manifest.version = \"$VERSION\";
}

manifest.pub_date = new Date().toISOString();
manifest.platforms[\"$PLATFORM\"] = {
    signature: process.env.UNKVOID_SIGNATURE,
    url: \"$BASE/$NAME\",
};

fs.writeFileSync(path, JSON.stringify(manifest, null, 2));
console.log(\"[INFO] manifesto:\", manifest.version, Object.keys(manifest.platforms).join(\", \"));
'"
