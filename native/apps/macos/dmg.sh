#!/bin/sh
# Empacota o build/Unkvoid.app num .dmg com o atalho para Aplicativos, do jeito que o site
# entrega (`darwin-aarch64-dmg`). Sem Developer ID o .app vai assinado ad-hoc: na primeira
# abertura a pessoa libera em Ajustes do Sistema › Privacidade e Segurança › "Abrir mesmo assim".
#
#   ./dmg.sh                         # gera build/Unkvoid_<versão>_aarch64.dmg (roda o bundle.sh antes)
#   RELEASE_SECRET=… ../desktop/publish-release.sh darwin-aarch64-dmg build/Unkvoid_<versão>_aarch64.dmg
set -eu

cd "$(dirname "$0")"

[ -d build/Unkvoid.app ] || ./bundle.sh "$@"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1)
staging=build/dmg
image="build/Unkvoid_${version}_aarch64.dmg"

rm -rf "$staging" "$image"
mkdir -p "$staging"
cp -R build/Unkvoid.app "$staging/"
ln -s /Applications "$staging/Applications"

hdiutil create -quiet -volname Unkvoid -srcfolder "$staging" -ov -format UDZO "$image"
rm -rf "$staging"

echo "pronto: $image"
