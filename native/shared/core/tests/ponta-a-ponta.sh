#!/usr/bin/env bash
# Transmitir e assistir de ponta a ponta no Linux, contra o SFU de verdade, sem tela nem som de
# verdade: um Xvfb mostra um vídeo de sincronia (clarão branco + bipe de 1 kHz a cada segundo), um
# servidor de som com saída nula toca o som dele, um SFU sobe local, e os testes do
# `native/shared/core/tests/live_room.rs` transmitem e assistem no mesmo processo. No fim, a queda
# de rede de 10 s de cada lado, com o `iptables` cortando só um dos dois processos.
#
# Roda num Ubuntu 24.04 ou Debian 12 com os pacotes do docs/VERIFICAR-WINDOWS-LINUX.md, como o seu
# usuário (o `cargo` e o `pnpm` são os dele). Só a queda de rede pede root, para o `iptables` e um
# usuário de teste: o script chama `sudo` para isso (a senha é pedida uma vez, no começo). Com
# SEM_REDE=1, ou sem `sudo`, a queda de rede é pulada.
#
#   native/shared/core/tests/ponta-a-ponta.sh                      # tudo
#   SEM_REDE=1 native/shared/core/tests/ponta-a-ponta.sh           # sem a queda de rede
#   SFU_DIR=/outro/checkout/sfu native/shared/core/tests/ponta-a-ponta.sh   # outro SFU
#
# Variáveis: SFU_DIR (o `sfu/` do repo), SFU_PORT (3300; não é a 3000 de quem desenvolve),
# SFU_MEDIA_PORT (43000) e SFU_PLAIN_PORT (44000), TELA (:99), PASTA (/tmp/unkvoid-ponta-a-ponta),
# e o CARGO_TARGET_DIR de sempre.
#
# A máquina volta como estava: a saída e a entrada de som padrão voltam às de antes, os módulos de
# som que ele carregou saem, o servidor de som que ele subiu desce, e o usuário e as regras do
# `iptables` da queda de rede são apagados — também se ele for interrompido no meio.
#
# Sai com 1 se qualquer caso falhar. Os logs ficam em $PASTA.
set -uo pipefail

cd "$(dirname "$0")/../../.." || exit 1
NATIVE="$PWD"
SFU_DIR="${SFU_DIR:-$NATIVE/../sfu}"
SFU_PORT="${SFU_PORT:-3300}"
SFU_MEDIA_PORT="${SFU_MEDIA_PORT:-43000}"
SFU_PLAIN_PORT="${SFU_PLAIN_PORT:-44000}"
TELA="${TELA:-:99}"
PASTA="${PASTA:-/tmp/unkvoid-ponta-a-ponta}"
SEGREDO="ponta-a-ponta-segredo-de-teste-com-mais-de-32-caracteres"
SFU_URL="ws://127.0.0.1:$SFU_PORT/sfu"
TARGET="${CARGO_TARGET_DIR:-$NATIVE/target}"
USUARIO_REDE=unkvoidrede
# Root só para a queda de rede; quem já é root não precisa do `sudo`.
if [ "$(id -u)" = 0 ]; then COMO_ROOT=(); else COMO_ROOT=(sudo); fi
# Na saída, sem pedir senha: se o `sudo` esqueceu, o que sobrar é avisado em vez de travar.
if [ "$(id -u)" = 0 ]; then NA_SAIDA=(); else NA_SAIDA=(sudo -n); fi
# O binário da queda de rede roda como outro usuário: fica numa pasta que ele alcança, fora da
# $PASTA (que pode estar dentro de uma pasta privada), e sai no fim.
REDE_DIR=""
FALHAS=0

mkdir -p "$PASTA" && chmod 755 "$PASTA"
anuncia() { printf '\n=== %s ===\n' "$1"; }
passou()  { printf 'PASSOU: %s\n' "$1"; }
falhou()  { printf 'FALHOU: %s\n' "$1"; FALHAS=$((FALHAS + 1)); }
# Os testes que falharam, pela lista do fim do `cargo test` (com `--nocapture` a linha do teste
# sai misturada com o que ele imprime).
falharam() {
    local nomes
    nomes=$(sed -n '/^failures:$/,/^test result/p' "$1" | grep -E '^    [a-z_]+$' | tr -d ' ' | tr '\n' ' ')
    echo "${nomes:-não chegou a rodar (compilação? ver $1)}"
}

FILHOS=()
MODULOS=()
SOM_NOSSO=0
SAIDA_ANTES=""
ENTRADA_ANTES=""
USUARIO_NOSSO=0

REDE=0

# A regra que marca os pacotes do usuário da queda e as que os derrubam.
regras_da_rede() {
    while "${NA_SAIDA[@]}" iptables -D OUTPUT -m connmark --mark 7 -j DROP 2>/dev/null; do :; done
    while "${NA_SAIDA[@]}" iptables -D INPUT -m connmark --mark 7 -j DROP 2>/dev/null; do :; done
    while "${NA_SAIDA[@]}" iptables -D OUTPUT -m owner --uid-owner "$USUARIO_REDE" -j CONNMARK --set-mark 7 2>/dev/null; do :; done
}

encerra() {
    for pid in "${FILHOS[@]}"; do kill "$pid" 2>/dev/null; done

    if [ -n "$REDE_DIR" ]; then
        "${NA_SAIDA[@]}" pkill -f "$REDE_DIR/room" 2>/dev/null
        rm -rf "$REDE_DIR"
    fi

    if [ "$REDE" = 1 ]; then
        regras_da_rede
    fi

    if [ "$USUARIO_NOSSO" = 1 ] && ! "${NA_SAIDA[@]}" userdel -r "$USUARIO_REDE" >/dev/null 2>&1; then
        echo "AVISO: o usuário $USUARIO_REDE ficou; apague com: sudo userdel -r $USUARIO_REDE"
    fi

    # O sink do som da tela, se um teste caiu com ele de pé (`unkvoid_share_<pid>_<n>`, pid já
    # morto); o de um Unkvoid aberto na máquina fica.
    pactl list modules short 2>/dev/null \
        | awk '$2 == "module-combine-sink" { for (i = 3; i <= NF; i++) if ($i ~ /^sink_name=unkvoid_share_[0-9]+_/) { split(substr($i, 25), partes, "_"); print $1, partes[1] } }' \
        | while read -r modulo pid; do
            [ -d "/proc/$pid" ] || pactl unload-module "$modulo" >/dev/null 2>&1
        done

    [ -n "$SAIDA_ANTES" ] && pactl set-default-sink "$SAIDA_ANTES" >/dev/null 2>&1
    [ -n "$ENTRADA_ANTES" ] && pactl set-default-source "$ENTRADA_ANTES" >/dev/null 2>&1

    # De trás para a frente: a fonte virtual depende do sink nulo.
    for ((indice = ${#MODULOS[@]} - 1; indice >= 0; indice--)); do
        pactl unload-module "${MODULOS[$indice]}" >/dev/null 2>&1
    done

    if [ "$SOM_NOSSO" = 1 ]; then
        pulseaudio --kill >/dev/null 2>&1
    fi
}
trap encerra EXIT
trap 'exit 130' INT TERM

anuncia "O que o script precisa"
faltam=""
for programa in cargo pnpm node Xvfb xdpyinfo pactl ffmpeg gst-launch-1.0 curl; do
    command -v "$programa" >/dev/null || faltam="$faltam $programa"
done
if [ -n "$faltam" ]; then
    falhou "faltam no PATH:$faltam (os pacotes e as ferramentas do §2.1 do docs/VERIFICAR-WINDOWS-LINUX.md)"
    exit 1
fi
if ! cargo build -q -p core-app --tests --examples >"$PASTA/build.log" 2>&1; then
    falhou "o núcleo não compilou (ver $PASTA/build.log)"
    exit 1
fi
passou "as ferramentas estão no PATH e o núcleo compilou"

if [ "${SEM_REDE:-0}" != 1 ] && command -v iptables >/dev/null && "${COMO_ROOT[@]}" true; then
    REDE=1
fi

anuncia "A tela, o som e o vídeo de sincronia"
Xvfb "$TELA" -screen 0 1280x720x24 >"$PASTA/xvfb.log" 2>&1 &
FILHOS+=($!)
export DISPLAY="$TELA"
sleep 2
xdpyinfo >/dev/null 2>&1 && passou "a tela $TELA respondeu" || falhou "a tela $TELA não subiu (outra tela já usa $TELA? troque com TELA=:98)"

if ! pactl info >/dev/null 2>&1; then
    pulseaudio --start --exit-idle-time=-1 >"$PASTA/pulse.log" 2>&1 && SOM_NOSSO=1
fi
SAIDA_ANTES="$(pactl get-default-sink 2>/dev/null)"
ENTRADA_ANTES="$(pactl get-default-source 2>/dev/null)"
modulo=$(pactl load-module module-null-sink sink_name=unkvoid_teste_saida 2>/dev/null) && MODULOS+=("$modulo")
modulo=$(pactl load-module module-virtual-source source_name=unkvoid_teste_microfone master=unkvoid_teste_saida.monitor 2>/dev/null) && MODULOS+=("$modulo")
pactl set-default-sink unkvoid_teste_saida >/dev/null 2>&1
pactl set-default-source unkvoid_teste_microfone >/dev/null 2>&1
[ "$(pactl get-default-sink 2>/dev/null)" = unkvoid_teste_saida ] && passou "o servidor de som respondeu (saída nula, a de antes volta no fim)" || falhou "o servidor de som não subiu"

if [ ! -s "$PASTA/sync.mkv" ]; then
    ffmpeg -hide_banner -loglevel error -y \
        -f lavfi -i "color=c=black:s=1280x720:r=30:d=120,drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='lt(mod(t,1),0.1)',drawtext=fontfile=/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf:text='%{frame_num}':fontsize=96:fontcolor=gray:x=40:y=40" \
        -f lavfi -i "aevalsrc='if(lt(mod(t\,1)\,0.1)\,0.8*sin(2*PI*1000*t)\,0)|if(lt(mod(t\,1)\,0.1)\,0.8*sin(2*PI*1000*t)\,0)':s=48000:d=120" \
        -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a pcm_s16le -shortest "$PASTA/sync.mkv"
fi

( while true; do
    gst-launch-1.0 -q filesrc location="$PASTA/sync.mkv" ! matroskademux name=d \
        d.video_0 ! queue ! decodebin ! videoconvert ! ximagesink force-aspect-ratio=false \
        d.audio_0 ! queue ! decodebin ! audioconvert ! audioresample ! pulsesink device=unkvoid_teste_saida
  done ) >"$PASTA/player.log" 2>&1 &
FILHOS+=($!)
sleep 3
pactl list sink-inputs short | grep -q . && passou "o vídeo de sincronia toca na tela e no som" || falhou "o player não abriu"

anuncia "O SFU ($SFU_DIR, porta $SFU_PORT)"
( cd "$SFU_DIR" && pnpm install --frozen-lockfile >"$PASTA/sfu-install.log" 2>&1 && pnpm run build >>"$PASTA/sfu-install.log" 2>&1 ) || falhou "o SFU não compilou"
( cd "$SFU_DIR" && SFU_SECRET="$SEGREDO" SFU_PORT="$SFU_PORT" SFU_MEDIA_PORT="$SFU_MEDIA_PORT" SFU_PLAIN_PORT="$SFU_PLAIN_PORT" \
    SFU_WORKERS=2 SFU_CONNECTIONS_PER_MINUTE=1000 SFU_PLAIN_PORTS=32 exec node dist/server.js ) >"$PASTA/sfu.log" 2>&1 &
FILHOS+=($!)
sleep 3
curl -sf "http://127.0.0.1:$SFU_PORT/health" >/dev/null && passou "o SFU respondeu no /health" || falhou "o SFU não subiu (a porta $SFU_PORT está livre?)"

anuncia "Transmitir e assistir (live_room.rs)"
if UNKVOID_SFU="$SFU_URL" SFU_SECRET="$SEGREDO" UNKVOID_CAMERA_SOURCE="videotestsrc is-live=true pattern=ball" \
    cargo test -p core-app --test live_room -- --ignored --test-threads=1 --nocapture \
    --skip late_viewers_see --skip five_percent_lost >"$PASTA/live_room.log" 2>&1; then
    passou "$(grep -oE '[0-9]+ passed' "$PASTA/live_room.log" | head -1 | cut -d' ' -f1) cenários de sala viva"
else
    falhou "a sala viva: $(falharam "$PASTA/live_room.log")"
fi
grep -E 'primeira imagem|depois da troca|a imagem voltou|blocos de voz' "$PASTA/live_room.log"

anuncia "O mesmo com 3% de perda na chegada (UNKVOID_LOSS)"
if UNKVOID_SFU="$SFU_URL" UNKVOID_LOSS=3 cargo test -p core-app --test live_room a_late_viewer -- --ignored --nocapture >"$PASTA/live_room-perda.log" 2>&1; then
    passou "imagem e som em sincronia com perda"
else
    falhou "a sincronia com perda"
fi
grep -E 'primeira imagem' "$PASTA/live_room-perda.log"

anuncia "Quadro-chave de quem entra e 5% de perda, com o GOP de 4 s do Windows"
if UNKVOID_SFU="$SFU_URL" UNKVOID_KEYFRAME_SECONDS=4 UNKVOID_CAMERA_SOURCE="videotestsrc is-live=true pattern=ball" \
    cargo test -p core-app --test live_room -- --ignored --test-threads=1 --nocapture late_viewers_see five_percent_lost >"$PASTA/live_room-gop.log" 2>&1; then
    passou "quem entra vê em até 1 s, e 5% de perda não param a imagem 1 s"
else
    falhou "o quadro-chave ou a perda: $(falharam "$PASTA/live_room-gop.log")"
fi
grep -E 'primeira imagem de quem entra|5% de perda' "$PASTA/live_room-gop.log"

if [ "$REDE" != 1 ]; then
    echo "AVISO: queda de rede pulada (SEM_REDE=1, ou sem sudo, ou sem iptables)"
else
    anuncia "Queda de rede de 10 s"
    # A senha de novo, se o `sudo` esqueceu nos minutos dos testes: daqui em diante ele roda atrás.
    [ "${#COMO_ROOT[@]}" -gt 0 ] && sudo -v
    REDE_DIR="$(mktemp -d /tmp/unkvoid-rede.XXXXXX)" && chmod 755 "$REDE_DIR"
    install -m 755 "$TARGET/debug/examples/room" "$REDE_DIR/room"
    if ! id "$USUARIO_REDE" >/dev/null 2>&1; then
        "${COMO_ROOT[@]}" useradd -m -s /bin/bash "$USUARIO_REDE" && USUARIO_NOSSO=1
    fi

    for lado in transmite assiste; do
        sala="rede${lado}$(date +%s | tail -c 5)"
        "${COMO_ROOT[@]}" iptables -I OUTPUT 1 -m owner --uid-owner "$USUARIO_REDE" -j CONNMARK --set-mark 7
        if [ "$lado" = transmite ]; then
            "${COMO_ROOT[@]}" runuser -u "$USUARIO_REDE" -- env DISPLAY="$TELA" "$REDE_DIR/room" "$SFU_URL" "$sala" share 45 >"$PASTA/rede-$lado-share.log" 2>&1 &
            transmite=$!
            sleep 5
            "$REDE_DIR/room" "$SFU_URL" "$sala" watch 38 >"$PASTA/rede-$lado-watch.log" 2>&1 &
            assiste=$!
        else
            DISPLAY="$TELA" "$REDE_DIR/room" "$SFU_URL" "$sala" share 45 >"$PASTA/rede-$lado-share.log" 2>&1 &
            transmite=$!
            sleep 5
            "${COMO_ROOT[@]}" runuser -u "$USUARIO_REDE" -- "$REDE_DIR/room" "$SFU_URL" "$sala" watch 38 >"$PASTA/rede-$lado-watch.log" 2>&1 &
            assiste=$!
        fi
        sleep 10
        "${COMO_ROOT[@]}" iptables -I OUTPUT 2 -m connmark --mark 7 -j DROP
        "${COMO_ROOT[@]}" iptables -I INPUT 1 -m connmark --mark 7 -j DROP
        sleep 10
        "${COMO_ROOT[@]}" iptables -D OUTPUT -m connmark --mark 7 -j DROP
        "${COMO_ROOT[@]}" iptables -D INPUT -m connmark --mark 7 -j DROP
        # Só os dois lados: um `wait` sem argumento esperaria também o Xvfb e o SFU, que não acabam.
        wait "$transmite" "$assiste"
        regras_da_rede

        # Os segundos com imagem depois da volta: a partir de 6 s depois dela, todos têm de ter.
        voltou=$(awk '/^t=/ { gsub(/s/, "", $2); if ($2 + 0 >= 26 && $4 + 0 >= 20) bons++; if ($2 + 0 >= 26) todos++ } END { printf "%d/%d", bons, todos }' "$PASTA/rede-$lado-watch.log")
        bons=${voltou%/*}; todos=${voltou#*/}
        if [ "$todos" -gt 0 ] && [ "$bons" -eq "$todos" ]; then
            passou "a imagem voltou depois de cair a rede de quem $lado ($voltou segundos bons)"
        else
            falhou "a imagem não voltou depois de cair a rede de quem $lado ($voltou segundos bons)"
        fi
    done
fi

anuncia "Resultado"
[ "$FALHAS" -eq 0 ] && echo "Tudo passou." || echo "$FALHAS caso(s) falharam; os logs estão em $PASTA."
exit $((FALHAS > 0))
