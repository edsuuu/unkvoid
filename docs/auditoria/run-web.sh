#!/usr/bin/env bash
# Roda os testes de reprodução do Laravel da auditoria de transmitir e assistir
# (docs/auditoria-transmissao.md) e apaga a cópia no fim, passe ou falhe. Os testes moram aqui,
# e não em web/tests, porque afirmam o defeito de hoje: dentro da suíte eles travariam a correção.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
web="$here/../../web"
target="$web/tests/Feature/Servers/VoiceAuditReproTest.php"

cp "$here/web/VoiceAuditReproTest.php" "$target"
trap 'rm -f "$target"' EXIT

cd "$web"

if [[ -x vendor/bin/pest ]]; then
    vendor/bin/pest tests/Feature/Servers/VoiceAuditReproTest.php --testdox
else
    php vendor/pestphp/pest/bin/pest tests/Feature/Servers/VoiceAuditReproTest.php --testdox
fi
