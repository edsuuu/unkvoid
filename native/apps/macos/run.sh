#!/bin/sh
# O app nativo do macOS de ponta a ponta: o núcleo em Rust e a janela em Swift.
#
#   ./run.sh            compila o núcleo e abre o app
#   ./run.sh test       os testes do núcleo (Rust) e do app (Swift), em série
#   ./run.sh app        monta o Unkvoid.app e o abre (permissões no nome do Unkvoid)
#
# O app fala com o Laravel em UNKVOID_SERVER (http://127.0.0.1:8000) e pega de lá o endereço
# do SFU. Sem o Laravel ele ainda abre sala por código, no SFU de UNKVOID_SFU
# (ws://127.0.0.1:3000/sfu). Como subir os dois está no CLAUDE.md da raiz.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
native=$(cd "$here/../.." && pwd)
server=${UNKVOID_SERVER:-http://127.0.0.1:8000}
export UNKVOID_SERVER="$server"

for tool in cargo swift; do
    command -v "$tool" >/dev/null || { echo "falta o '$tool' no PATH"; exit 1; }
done

reachable() {
    curl -s -o /dev/null -m 2 "$1"
}

reachable "$server/api/config" || echo "aviso: o Laravel não respondeu em $server — só a sala por código vai funcionar (cd web && composer dev)"
reachable "http://127.0.0.1:3000/health" || echo "aviso: nenhum SFU em 127.0.0.1:3000 (cd sfu && pnpm run dev)"

case "${1:-run}" in
    run)
        (cd "$native" && cargo build -p core-app)
        cd "$here" && exec swift run Unkvoid
        ;;
    test)
        (cd "$native" && cargo test -p core-app -p media -p storage)
        (cd "$native" && cargo build -p core-app)
        # Em série: os testes dividem uma pasta de estado e as contas de teste.
        cd "$here" && exec swift test --no-parallel
        ;;
    app)
        "$here/bundle.sh"
        exec open "$here/build/Unkvoid.app"
        ;;
    *)
        echo "uso: ./run.sh [run|test|app]"
        exit 1
        ;;
esac
