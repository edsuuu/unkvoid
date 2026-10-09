#!/usr/bin/env bash
# Transmitir e assistir de ponta a ponta no Linux, contra o SFU de verdade, sem tela nem som de
# verdade: um Xvfb mostra um vídeo de sincronia (clarão branco + bipe de 1 kHz a cada segundo), um
# PulseAudio com saída nula toca o som dele, o SFU do repo sobe local, e os testes do
# `native/shared/core/tests/live_room.rs` transmitem e assistem no mesmo processo. No fim, a queda
# de rede de 10 s de cada lado, com o `iptables` cortando só um dos dois processos.
#
# Roda num Ubuntu 24.04 ou Debian 12 com os pacotes do docs/VERIFICAR-WINDOWS-LINUX.md, como
# root (o `iptables` e o usuário da queda de rede pedem). Sem root, a queda de rede é pulada.
#
#   native/shared/core/tests/ponta-a-ponta.sh            # tudo
#   SEM_REDE=1 native/shared/core/tests/ponta-a-ponta.sh # sem a queda de rede
#
# Sai com 1 se qualquer caso falhar. Os logs ficam em $PASTA (padrão /tmp/unkvoid-ponta-a-ponta).
set -uo pipefail

cd "$(dirname "$0")/../../.." || exit 1
NATIVE="$PWD"
SFU_DIR="$NATIVE/../sfu"
PASTA="${PASTA:-/tmp/unkvoid-ponta-a-ponta}"
SEGREDO="ponta-a-ponta-segredo-de-teste-com-mais-de-32-caracteres"
SFU_URL="ws://127.0.0.1:3000/sfu"
FALHAS=0

mkdir -p "$PASTA" && chmod 755 "$PASTA"
anuncia() { printf '\n=== %s ===\n' "$1"; }
passou()  { printf 'PASSOU: %s\n' "$1"; }
falhou()  { printf 'FALHOU: %s\n' "$1"; FALHAS=$((FALHAS + 1)); }

FILHOS=()
encerra() {
    for pid in "${FILHOS[@]}"; do kill "$pid" 2>/dev/null; done
    pactl unload-module module-combine-sink >/dev/null 2>&1
}
trap encerra EXIT

anuncia "A tela, o som e o vídeo de sincronia"
Xvfb :99 -screen 0 1280x720x24 >"$PASTA/xvfb.log" 2>&1 &
FILHOS+=($!)
export DISPLAY=:99
sleep 2
xdpyinfo >/dev/null 2>&1 && passou "a tela :99 respondeu" || falhou "a tela :99 não subiu"

pulseaudio --start --exit-idle-time=-1 >"$PASTA/pulse.log" 2>&1
pactl load-module module-null-sink sink_name=fake >/dev/null 2>&1
pactl load-module module-virtual-source source_name=microfone master=fake.monitor >/dev/null 2>&1
pactl set-default-sink fake >/dev/null 2>&1
pactl set-default-source microfone >/dev/null 2>&1
pactl info >/dev/null 2>&1 && passou "o servidor de som respondeu" || falhou "o PulseAudio não subiu"

if [ ! -s "$PASTA/sync.mkv" ]; then
    ffmpeg -hide_banner -loglevel error -y \
        -f lavfi -i "color=c=black:s=1280x720:r=30:d=120,drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='lt(mod(t,1),0.1)',drawtext=fontfile=/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf:text='%{frame_num}':fontsize=96:fontcolor=gray:x=40:y=40" \
        -f lavfi -i "aevalsrc='if(lt(mod(t\,1)\,0.1)\,0.8*sin(2*PI*1000*t)\,0)|if(lt(mod(t\,1)\,0.1)\,0.8*sin(2*PI*1000*t)\,0)':s=48000:d=120" \
        -c:v libx264 -preset ultrafast -pix_fmt yuv420p -c:a pcm_s16le -shortest "$PASTA/sync.mkv"
fi

( while true; do
    gst-launch-1.0 -q filesrc location="$PASTA/sync.mkv" ! matroskademux name=d \
        d.video_0 ! queue ! decodebin ! videoconvert ! ximagesink force-aspect-ratio=false \
        d.audio_0 ! queue ! decodebin ! audioconvert ! audioresample ! pulsesink device=fake
  done ) >"$PASTA/player.log" 2>&1 &
FILHOS+=($!)
sleep 3
pactl list sink-inputs short | grep -q . && passou "o vídeo de sincronia toca na tela e no som" || falhou "o player não abriu"

anuncia "O SFU do repo"
( cd "$SFU_DIR" && pnpm install --frozen-lockfile >"$PASTA/sfu-install.log" 2>&1 && pnpm run build >>"$PASTA/sfu-install.log" 2>&1 ) || falhou "o SFU não compilou"
( cd "$SFU_DIR" && SFU_SECRET="$SEGREDO" SFU_WORKERS=2 SFU_CONNECTIONS_PER_MINUTE=1000 SFU_PLAIN_PORTS=32 exec node dist/server.js ) >"$PASTA/sfu.log" 2>&1 &
FILHOS+=($!)
sleep 3
curl -sf http://127.0.0.1:3000/health >/dev/null && passou "o SFU respondeu no /health" || falhou "o SFU não subiu"

anuncia "Transmitir e assistir (live_room.rs)"
if UNKVOID_SFU="$SFU_URL" SFU_SECRET="$SEGREDO" UNKVOID_CAMERA_SOURCE="videotestsrc is-live=true pattern=ball" \
    cargo test -p core-app --test live_room -- --ignored --test-threads=1 --nocapture >"$PASTA/live_room.log" 2>&1; then
    passou "$(grep -oE '[0-9]+ passed' "$PASTA/live_room.log" | head -1 | cut -d' ' -f1) cenários de sala viva"
else
    falhou "a sala viva: $(grep -E '\.\.\. FAILED' "$PASTA/live_room.log" | tr '\n' ' ')"
fi
grep -E 'primeira imagem|depois da troca|a imagem voltou|blocos de voz' "$PASTA/live_room.log"

anuncia "O mesmo com 3% de perda na chegada (UNKVOID_LOSS)"
if UNKVOID_SFU="$SFU_URL" UNKVOID_LOSS=3 cargo test -p core-app --test live_room a_late_viewer -- --ignored --nocapture >"$PASTA/live_room-perda.log" 2>&1; then
    passou "imagem e som em sincronia com perda"
else
    falhou "a sincronia com perda"
fi
grep -E 'primeira imagem' "$PASTA/live_room-perda.log"

if [ "${SEM_REDE:-0}" = 1 ] || [ "$(id -u)" != 0 ]; then
    echo "AVISO: queda de rede pulada (precisa de root e do iptables)"
else
    anuncia "Queda de rede de 10 s"
    cargo build -q -p core-app --example room || falhou "o exemplo room não compilou"
    install -m 755 "$NATIVE/target/debug/examples/room" "$PASTA/room"
    id unkvoidrede >/dev/null 2>&1 || useradd -m -s /bin/bash unkvoidrede

    for lado in transmite assiste; do
        sala="rede${lado}$(date +%s | tail -c 5)"
        iptables -I OUTPUT 1 -m owner --uid-owner unkvoidrede -j CONNMARK --set-mark 7
        if [ "$lado" = transmite ]; then
            runuser -u unkvoidrede -- env DISPLAY=:99 "$PASTA/room" "$SFU_URL" "$sala" share 45 >"$PASTA/rede-$lado-share.log" 2>&1 &
            transmite=$!
            sleep 5
            "$PASTA/room" "$SFU_URL" "$sala" watch 38 >"$PASTA/rede-$lado-watch.log" 2>&1 &
            assiste=$!
        else
            DISPLAY=:99 "$PASTA/room" "$SFU_URL" "$sala" share 45 >"$PASTA/rede-$lado-share.log" 2>&1 &
            transmite=$!
            sleep 5
            runuser -u unkvoidrede -- "$PASTA/room" "$SFU_URL" "$sala" watch 38 >"$PASTA/rede-$lado-watch.log" 2>&1 &
            assiste=$!
        fi
        sleep 10
        iptables -I OUTPUT 2 -m connmark --mark 7 -j DROP
        iptables -I INPUT 1 -m connmark --mark 7 -j DROP
        sleep 10
        iptables -D OUTPUT -m connmark --mark 7 -j DROP
        iptables -D INPUT -m connmark --mark 7 -j DROP
        # Só os dois lados: um `wait` sem argumento esperaria também o Xvfb e o SFU, que não acabam.
        wait "$transmite" "$assiste"
        iptables -D OUTPUT -m owner --uid-owner unkvoidrede -j CONNMARK --set-mark 7

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
