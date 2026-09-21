#!/bin/sh
# Monta o Unkvoid.app a partir do build de release. Um executável solto funciona para
# desenvolver, mas a permissão de tela, microfone e câmera fica no nome do terminal que o
# abriu; num .app ela é do Unkvoid, e o ícone aparece no Dock.
#
#   ./bundle.sh            # gera build/Unkvoid.app
#   ./bundle.sh --sign "Developer ID Application: …"
set -eu

cd "$(dirname "$0")"

(cd ../.. && cargo build --release -p core-app)
UNKVOID_CORE=release swift build -c release

app=build/Unkvoid.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp .build/release/Unkvoid "$app/Contents/MacOS/Unkvoid"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' ../../Cargo.toml | head -1)

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>Unkvoid</string>
    <key>CFBundleIdentifier</key><string>com.unkvoid.desktop</string>
    <key>CFBundleName</key><string>Unkvoid</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${version:-0.0.0}</string>
    <key>CFBundleVersion</key><string>${version:-0.0.0}</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSMicrophoneUsageDescription</key><string>O Unkvoid usa o microfone para você falar nos canais de voz.</string>
    <key>NSCameraUsageDescription</key><string>O Unkvoid usa a câmera quando você a liga num canal de voz.</string>
</dict>
</plist>
PLIST

identity="-"

if [ "${1:-}" = "--sign" ]; then
    identity="$2"
fi

codesign --force --deep --options runtime --sign "$identity" "$app"

echo "pronto: $app"
