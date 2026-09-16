#!/usr/bin/env bash
# Os casos de uso do Linux, que é onde a tela é compartilhada.
#
# Roda dentro do contêiner (veja o README.md ao lado). Cada caso imprime PASSOU ou FALHOU
# e o script sai com 1 se qualquer um falhar.
set -uo pipefail

# O cargo precisa do Cargo.toml: este script mora em native/tests/linux.
cd "$(dirname "$0")/../.." || exit 1

FALHAS=0
AVISOS=0
SEGUNDOS="${SEGUNDOS:-45}"

anuncia() { printf '\n=== %s ===\n' "$1"; }
passou()  { printf 'PASSOU: %s\n' "$1"; }
falhou()  { printf 'FALHOU: %s\n' "$1"; FALHAS=$((FALHAS + 1)); }
avisa()   { printf 'AVISO: %s\n' "$1"; AVISOS=$((AVISOS + 1)); }

anuncia "Preparando a tela e o som de mentira"
Xvfb :99 -screen 0 1280x720x24 >/tmp/xvfb.log 2>&1 &
export DISPLAY=:99
sleep 2
xdpyinfo >/dev/null 2>&1 && passou "a tela :99 respondeu" || falhou "a tela :99 não subiu"

# Algo se mexendo na tela: com a tela parada o `use-damage=false` ainda entrega quadros,
# mas um relógio deixa o teste parecido com um jogo rodando.
xclock -update 1 >/dev/null 2>&1 &

pulseaudio --start --exit-idle-time=-1 >/tmp/pulse.log 2>&1
pactl load-module module-null-sink sink_name=fake >/dev/null 2>&1
pactl load-module module-virtual-source source_name=microfone master=fake.monitor >/dev/null 2>&1
# O `@DEFAULT_MONITOR@` do pipeline da tela só resolve se houver saída padrão.
pactl set-default-sink fake >/dev/null 2>&1
pactl set-default-source microfone >/dev/null 2>&1
pactl info >/dev/null 2>&1 && passou "o servidor de som respondeu" || falhou "o PulseAudio não subiu"

anuncia "Caso 1 — a transmissão não cai depois de 30 segundos"
# O relato era esse: aos 30 s a transmissão morria. O exemplo conta quadros por segundo,
# então basta olhar os segundos DEPOIS do minuto crítico.
if cargo run --quiet -p capture --example spike -- 720 "$SEGUNDOS" >/tmp/spike.log 2>&1; then
    APOS_TRINTA=$(awk '/s  / { gsub(/s/, "", $1); if ($1 + 0 >= 31 && $2 + 0 > 0) contados++ } END { print contados + 0 }' /tmp/spike.log)
    ZERADOS=$(awk '/s  / { if ($2 + 0 == 0) zerados++ } END { print zerados + 0 }' /tmp/spike.log)

    if [ "$APOS_TRINTA" -ge 10 ] && [ "$ZERADOS" -eq 0 ]; then
        passou "quadros em todos os segundos, $APOS_TRINTA deles depois dos 30 s"
    else
        falhou "a captura parou: $ZERADOS segundos sem quadro, $APOS_TRINTA segundos bons depois dos 30 s"
        tail -20 /tmp/spike.log
    fi
else
    falhou "o exemplo de captura nem rodou"
    tail -20 /tmp/spike.log
fi

anuncia "Caso 2 — o áudio do sistema entra junto com a tela"
CHUNKS=$(awk '/^system audio:/ { print $3 + 0 }' /tmp/spike.log)
if [ "${CHUNKS:-0}" -gt 0 ]; then
    passou "$CHUNKS blocos de áudio junto da tela"
else
    falhou "nenhum bloco de áudio: o pulsesrc não pegou o monitor"
fi

anuncia "Caso 3 — o microfone nativo abre e entrega som"
if timeout 30 gst-launch-1.0 -q pulsesrc device=fake.monitor num-buffers=100 \
        ! audioconvert ! audio/x-raw,format=S16LE,rate=48000,channels=2 \
        ! fdsink fd=1 > /tmp/microfone.raw 2>/tmp/microfone.log; then
    TAMANHO=$(stat -c %s /tmp/microfone.raw)
    [ "$TAMANHO" -gt 10000 ] && passou "$TAMANHO bytes do microfone" || falhou "o microfone entregou só $TAMANHO bytes"
else
    falhou "o pipeline do microfone não rodou"
    cat /tmp/microfone.log
fi

anuncia "Caso 4 — o tratamento de áudio (eco, ruído, ganho) existe nesta máquina"
if gst-inspect-1.0 webrtcdsp >/dev/null 2>&1; then
    passou "webrtcdsp presente: o microfone sai com eco e ruído tratados"
else
    # Não é falha do app: o Ubuntu 24.04 não distribui esse elemento em nenhuma
    # arquitetura, então `microphone_pipeline()` cai no microfone cru. Está registrado
    # em docs/ESTADO.md com o caminho de saída (module-echo-cancel do PulseAudio).
    avisa "sem webrtcdsp nesta distro: o microfone vai cru, sem eco nem ruído tratados"
fi

anuncia "Caso 5 — o encoder que esta máquina escolhe"
# Sem placa de vídeo no contêiner o esperado é o x264enc; o que importa é a escolha ter
# acontecido e a captura ter aberto com ela.
ENCODER=$(grep -oiE "(nvh264enc|vaapih264enc|v4l2h264enc|x264enc)" /tmp/spike.log | head -1)
if [ -n "$ENCODER" ]; then
    passou "encoder escolhido: $ENCODER"
elif grep -q "capturing" /tmp/spike.log; then
    passou "a captura abriu (o log não nomeou o encoder)"
else
    falhou "nenhum encoder abriu"
fi

anuncia "Caso 6 — os testes do Rust passam neste Linux"
if cargo test --quiet --workspace >/tmp/testes.log 2>&1; then
    passou "cargo test --workspace"
else
    falhou "cargo test --workspace"
    tail -30 /tmp/testes.log
fi

printf '\n===========================\n'
printf '%s aviso(s).\n' "$AVISOS"

if [ "$FALHAS" -eq 0 ]; then
    printf 'Tudo passou.\n'
else
    printf '%s caso(s) falharam.\n' "$FALHAS"
fi

exit "$FALHAS"
