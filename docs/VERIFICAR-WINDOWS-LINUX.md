# Verificar o app nativo numa máquina Windows e numa Linux de verdade

Para o próximo agente (ou pessoa) que vai compilar e provar o app Slint (`native/apps/windows`, o
mesmo crate nos dois sistemas) e o núcleo (`native/shared`) em hardware de verdade. Escrito em
09/10/2026 depois de duas rodadas num contêiner Ubuntu 24.04 sem placa de vídeo, sem webcam e
sem Windows; o relatório de cada rodada está na descrição do PR #53.

Cada bloco abaixo roda como está, de cima para baixo, e começa indo para a pasta dele (`cd` no
começo), então tanto faz onde o bloco anterior deixou o terminal. Os marcados **(comprovado)**
rodaram assim no contêiner, e o roteiro inteiro do Linux foi seguido do zero por outro agente. Os do Windows marcados **(derivado)** saíram
da compilação cruzada (`x86_64-pc-windows-gnu`) e da leitura do código: ninguém os rodou num
Windows.

O que está aqui:

1. [o que já foi provado e o que só a máquina prova](#1-o-que-já-foi-provado-e-o-que-falta)
2. [Linux: pré-requisitos, build, testes e execução](#2-linux)
3. [Windows: pré-requisitos, build, testes e execução](#3-windows)
4. [o roteiro manual, igual nos dois sistemas, com o resultado esperado em cada passo](#4-roteiro-manual)
5. [onde ficam os logs e como reportar](#5-logs-e-como-reportar)

## 1. O que já foi provado, e o que falta

| Peça | Provado no contêiner | Só a máquina de verdade prova |
|---|---|---|
| Empacotar e desempacotar H.264 (FU-A, STAP-A, marker, volta do número e do relógio) | testes do `media` | — |
| Perda, reenvio (NACK/RTX), pedido de quadro-chave (PLI), jitter buffer, RR de quem assiste e DLRR de quem transmite | testes do `media`, o E2E com `UNKVOID_LOSS` e o harness do SFU (#54) | perda real de internet (Wi-Fi, 4G) |
| Quem entra atrasado vê a tela e a câmera em até 1 s | E2E com o GOP de 4 s do Windows (`UNKVOID_KEYFRAME_SECONDS=4`) e o harness do SFU | o encoder da placa atendendo o PLI no quadro seguinte |
| Captura de tela no Linux (X11, `ximagesrc`) + x264 | E2E: 1280x720 a 30 fps | Wayland pelo portal (o seletor do sistema), encoders de placa (`nvh264enc`, `vah264enc`) |
| Decodificar no Linux (`avdec_h264` dentro do processo) | testes + o app no Xvfb | 1080p60 e 4K numa CPU de verdade |
| Som da tela no Linux (sink combinado) | testes contra o PulseAudio 16.1 **e** o `pipewire-pulse` 1.0.5, inclusive o sink que um app morto deixou | o PipeWire de uma distro com sessão de verdade |
| Câmera no Linux | E2E com `videotestsrc` no lugar da webcam, e a câmera que volta sozinha depois de uma queda | uma webcam USB de verdade (`v4l2src`), e a escolha entre duas |
| Microfone | E2E (PCM empurrado pelo `speak`) | o `pulsesrc` do app com um microfone de verdade |
| Sincronia imagem/som | E2E: mediana de 23 a 68 ms (alvo < 80); o som da tela segue a espera da imagem com 40 ms de folga e emendas sem estalo | o ouvido e o olho, com o alto-falante e a placa de verdade |
| Queda de rede de 10 s | E2E: a imagem volta de 1 a 3 s depois da rede, dos dois lados | trocar de rede (Wi-Fi → cabo), reiniciar o roteador |
| Mover de canal | E2E com o `kick` assinado | pelo Laravel, com o botão do app |
| Windows inteiro (WGC, Desktop Duplication, Media Foundation, WASAPI) | só compila (clippy limpo no alvo Windows) | **tudo** — a seção 3 é o roteiro |

## 2. Linux

### 2.1 Pré-requisitos (comprovado em Ubuntu 24.04.5)

| O quê | Versão usada | Para quê |
|---|---|---|
| Rust | 1.97.0 + `clippy` | edição 2024 e `let`-chains |
| GStreamer | 1.24.2 (os pacotes do `apt` abaixo) | captura e encoder (dentro do processo), decodificador, som |
| Janela | `libfontconfig1-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libwayland-dev`, `libegl1`, `libgles2`, `libgl1-mesa-dri` | Slint (winit + femtovg) |
| GTK 4 | `libgtk-4-dev` | o `unkvoid-linux`, aposentado mas ainda no workspace: sem ele o `--workspace` não compila |
| Chaveiro | `libdbus-1-dev` | `shared/storage` (Secret Service) |
| Opus | `cmake` 3.28 (e `libopus-dev`) | o `opusic-sys` morre sem `cmake` com uma mensagem que não diz isso |
| Som | `pulseaudio-utils` (`pactl`, `paplay`); servidor PulseAudio 16.1 ou PipeWire (`pipewire-pulse`) | filtro do som da tela, saída e microfone |
| Para os testes sem tela | `xvfb`, `x11-utils`, `xdotool`, `imagemagick`, `ffmpeg` 6.1, `pulseaudio`, `iptables`, `curl` | E2E e capturas de tela |
| SFU local | Node 22.22, pnpm 10.28 (o mediasoup 3.26 baixa o worker pronto) | os testes contra o SFU |

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev \
  gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly gstreamer1.0-libav gstreamer1.0-x gstreamer1.0-pulseaudio gstreamer1.0-pipewire \
  libdbus-1-dev libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libgtk-4-dev \
  libegl1 libgles2 libgl1-mesa-dri build-essential cmake pkg-config libopus-dev pulseaudio-utils \
  xvfb x11-utils xdotool imagemagick ffmpeg pulseaudio iptables curl ca-certificates fonts-dejavu-core
# Rust, se ainda não houver
command -v cargo || { curl -sSf https://sh.rustup.rs | sh -s -- -y && . "$HOME/.cargo/env"; }
rustup component add clippy
# Node 22 e pnpm 10, se ainda não houver
command -v node || { curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash - && sudo apt-get install -y nodejs; }
command -v pnpm || sudo npm install -g pnpm@10
```

### 2.2 Build, lint e testes (comprovado)

```bash
cd "$(git rev-parse --show-toplevel)/native"
cargo build --workspace --exclude unkvoid-desktop --all-targets
cargo clippy --workspace --exclude unkvoid-desktop --all-targets -- -D warnings
cargo test --workspace --exclude unkvoid-desktop
```

`unkvoid-desktop` é o Tauri legado: fora daqui de propósito. O `unkvoid-linux` (GTK) foi
aposentado pelo dono, mas ainda compila e entra no workspace; quem não instalou o `libgtk-4-dev`
acrescenta `--exclude unkvoid-linux` aos três. Os dois apps geram um binário com o mesmo nome
(o `cargo` avisa `output filename collision`): o `target/debug/unkvoid` é o do último que
compilou, então rode o app sempre por `cargo run -p unkvoid-windows`.

Os testes que pedem GStreamer com plugins, servidor de som ou SFU no ar ficam `#[ignore]`:

```bash
cd "$(git rev-parse --show-toplevel)/native"
# GStreamer de verdade: encoder, keyframe por PLI, taxa no ar, relógio da captura, decodificador.
# Os do som ficam de fora aqui: são os do comando seguinte, um de cada vez.
cargo test -p capture -p media -- --ignored --skip linux_audio
# A sala contra um SFU de mentira: a câmera que volta depois de uma queda
cargo test -p core-app --test room -- --ignored
```

### 2.3 O som da tela contra o PulseAudio e o PipeWire (comprovado)

Precisa de um servidor de som com uma saída padrão. Numa máquina com som, os testes usam a dela
— mudos: o "jogo" de mentira toca em volume zero. Num contêiner, sobe um:

```bash
cd "$(git rev-parse --show-toplevel)/native"
pulseaudio --start --exit-idle-time=-1
pactl load-module module-null-sink sink_name=fake && pactl set-default-sink fake
cargo test -p capture --lib linux_audio -- --ignored --test-threads=1
pulseaudio --kill
```

`--test-threads=1`: dois testes subindo o sink combinado ao mesmo tempo derrubam o PulseAudio 16
(é bug dele, e é por isso que o app só sobe um). Os três: o jogo vai para o sink e a chamada não;
parar de compartilhar seis vezes com um jogo tocando não derruba o servidor; e o sink que um app
morto deixou com o jogo dentro sai na transmissão seguinte.

O mesmo no PipeWire (`sudo apt-get install -y pipewire pipewire-pulse wireplumber dbus`). Numa
sessão de desktop ele já está no ar; num contêiner:

```bash
cd "$(git rev-parse --show-toplevel)/native"
pulseaudio --kill 2>/dev/null                       # o PipeWire no lugar dele
# Num subshell: as variáveis do PipeWire de mentira não vazam para o terminal de quem roda.
(
    export XDG_RUNTIME_DIR=$(mktemp -d) && chmod 700 "$XDG_RUNTIME_DIR"
    dbus-daemon --session --address="unix:path=$XDG_RUNTIME_DIR/bus" --fork --print-pid >"$XDG_RUNTIME_DIR/dbus.pid"
    export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
    (pipewire >/dev/null 2>&1 &); sleep 1; (wireplumber >/dev/null 2>&1 &); sleep 1; (pipewire-pulse >/dev/null 2>&1 &); sleep 2
    pactl info | grep 'Server Name'                  # PulseAudio (on PipeWire 1.0.5)
    pactl load-module module-null-sink sink_name=fake && pactl set-default-sink fake
    cargo test -p capture --lib linux_audio -- --ignored --test-threads=1
    pkill -x pipewire-pulse; pkill -x wireplumber; pkill -x pipewire; kill "$(cat "$XDG_RUNTIME_DIR/dbus.pid")"
    rm -rf "$XDG_RUNTIME_DIR"
)
```

### 2.4 De ponta a ponta contra o SFU (comprovado)

Um script monta tudo — Xvfb, saída de som nula, um vídeo de clarão e bipe a cada segundo
tocando na tela e no som, o SFU — e roda os cenários do `native/shared/core/tests/live_room.rs`
e a queda de rede de 10 s de cada lado:

```bash
cd "$(git rev-parse --show-toplevel)"
native/shared/core/tests/ponta-a-ponta.sh               # tudo; a queda de rede pede a senha do sudo
SEM_REDE=1 native/shared/core/tests/ponta-a-ponta.sh    # sem a queda de rede, sem sudo
```

Rode como o seu usuário, não com `sudo` na frente: o `sudo` troca o `PATH` e a casa, e o `cargo`
e o `pnpm` somem (e o que compilasse ficaria com dono root). O script chama `sudo` só para o
`iptables` e o usuário de teste da queda de rede, e pede a senha uma vez. Leva ~3 min com o build
pronto e ~15 min do zero; antes de tudo ele confere as ferramentas no `PATH` e compila o núcleo,
e para ali se faltar alguma coisa.

A máquina volta como estava: a saída e a entrada de som padrão voltam às de antes, os módulos de
som que ele carregou saem, o PulseAudio que ele subiu (se não havia um) desce, e o usuário
`unkvoidrede` e as regras do `iptables` da queda de rede somem — também se ele for interrompido
no meio. O SFU sobe na porta 3300 (não na 3000 de quem desenvolve); outro SFU, outra porta e
outra pasta de logs: `SFU_DIR=/outro/sfu SFU_PORT=3400 PASTA=/tmp/outra ...`. A tela é a `:99`
(`TELA=:98` se ela estiver ocupada).

Cada cenário imprime o que mediu; o esperado (o que saiu no contêiner, sem placa):

| Cenário | Esperado |
|---|---|
| `a_late_viewer_sees_and_hears_the_screen_in_sync` | primeira imagem < 3 s depois de entrar (saiu ~0,5 s), ≥ 25 imagens/s, nenhuma parada > 500 ms, 1280x720, desvio A/V mediano < 80 ms (saiu 23–69 ms) |
| o mesmo com `UNKVOID_LOSS=3` | o mesmo; a espera da imagem cresce e o som acompanha (saiu 23–69 ms) |
| `the_microphone_reaches_the_room` | > 50 blocos de voz em 3 s (saiu 133) |
| `a_camera_is_watched_beside_the_screen` | ≥ 60 imagens da câmera em 5 s, 640x360, a tela continua |
| `stopping_and_sharing_again_brings_the_picture_back` | a imagem da tela nova em < 4 s (saiu ~1 s) |
| `a_quality_change_keeps_the_clock_and_the_picture` | 12–18 imagens/s depois de pedir 15, parada < 1,5 s, relógio do RTP descompassado < 150 ms |
| `a_moved_person_leaves_for_good_and_shares_in_the_new_room` | o aviso `moved` com o destino, a tela sai da origem e não volta sozinha, e transmite no destino |
| `late_viewers_see_the_screen_and_the_camera_within_a_second` (GOP de 4 s) | tela e câmera de cada atrasado — sozinho, dois a 300 ms, três juntos — em até 1 s |
| `five_percent_lost_on_the_way_in_never_holds_the_picture_for_a_second` (GOP de 4 s) | 5% de perda na chegada por 15 s: maior parada ≤ 1 s, ≥ 24 imagens/s (saiu 126–219 ms, 30 imagens/s, nenhum buraco largado) |
| queda de rede de quem transmite / de quem assiste | a imagem volta sozinha em até 3 s depois da rede (saiu no primeiro segundo; o script exige imagem em todo segundo a partir de 6 s depois da volta) |

Os testes um a um, sem o script: precisam de um SFU no ar, de uma tela X com algo mexendo
(`DISPLAY`) e de um servidor de som com saída padrão — numa sessão de desktop os dois últimos já
existem. O de sincronia (`a_late_viewer…`) espera, além disso, o vídeo do clarão e do bipe
tocando na tela e no som: o `sync.mkv` que o script gera em `$PASTA`. O SFU, na mesma porta do
script:

```bash
cd "$(git rev-parse --show-toplevel)/sfu"
pnpm install --frozen-lockfile && pnpm run build
SFU_SECRET=um-segredo-local-de-teste-com-mais-de-32-letras SFU_PORT=3300 SFU_MEDIA_PORT=43000 SFU_PLAIN_PORT=44000 \
  SFU_WORKERS=2 SFU_CONNECTIONS_PER_MINUTE=1000 SFU_PLAIN_PORTS=32 node dist/server.js >/tmp/sfu-teste.log 2>&1 &
echo $! >/tmp/sfu-teste.pid
```

Os testes (o da câmera e os do GOP de 4 s só existem no build de depuração, que é o do
`cargo test`), e o SFU desce no fim:

```bash
cd "$(git rev-parse --show-toplevel)/native"
export UNKVOID_SFU=ws://127.0.0.1:3300/sfu SFU_SECRET=um-segredo-local-de-teste-com-mais-de-32-letras
UNKVOID_CAMERA_SOURCE="videotestsrc is-live=true pattern=ball" \
  cargo test -p core-app --test live_room -- --ignored --test-threads=1 --nocapture --skip late_viewers_see --skip five_percent_lost
UNKVOID_KEYFRAME_SECONDS=4 UNKVOID_CAMERA_SOURCE="videotestsrc is-live=true pattern=ball" \
  cargo test -p core-app --test live_room -- --ignored --test-threads=1 --nocapture late_viewers_see five_percent_lost
kill "$(cat /tmp/sfu-teste.pid)"
```

E o medidor de sempre, um processo de cada lado (agora decodifica também no Linux):

```bash
cd "$(git rev-parse --show-toplevel)/native"
cargo run -p core-app --example room -- ws://127.0.0.1:3300/sfu sala-de-teste share 60
cargo run -p core-app --example room -- ws://127.0.0.1:3300/sfu sala-de-teste watch 30
```

### 2.5 O app (comprovado no Xvfb)

```bash
cd "$(git rev-parse --show-toplevel)/native"
UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid-windows                       # femtovg (OpenGL)
UNKVOID_SERVER=http://127.0.0.1:8000 SLINT_BACKEND=winit-software cargo run -p unkvoid-windows   # sem OpenGL
```

Sem o Laravel local, o app precisa de `/health` (o SFU) e `/api/config` (`{"sfu": "ws://…"}`)
no `UNKVOID_SERVER`: um servidorzinho que responda os dois basta para a sala por código. Num
Linux de verdade, prove também: `SLINT_BACKEND=winit-x11` numa sessão Wayland, e o portal
(`UNKVOID_CAPTURE=portal`).

Variáveis úteis: `UNKVOID_ENCODER=cpu` (pula o encoder da placa), `UNKVOID_CAPTURE=x11|portal`,
`UNKVOID_LOSS=<%>` (perda de propósito na chegada), `RUST_LOG=debug` (mais detalhe no log). Só
no build de depuração (`cargo run`, `cargo test`), como ferramenta de teste:
`UNKVOID_CAMERA_SOURCE=<origem do GStreamer>` (câmera sem webcam) e
`UNKVOID_KEYFRAME_SECONDS=<s>` (o GOP do Linux; 4 é o do Windows).

## 3. Windows

### 3.1 Pré-requisitos (derivado)

| O quê | Versão | Para quê |
|---|---|---|
| Windows | 11 23H2+ e 10 22H2 (as duas: no 10 o monitor sai pelo Desktop Duplication) | |
| Rust | 1.97.0, alvo `x86_64-pc-windows-msvc`, + `clippy` | |
| Visual Studio 2022 Build Tools | "Desenvolvimento para desktop com C++" (MSVC v143 + Windows 11 SDK) | linker e os C das dependências |
| CMake | 3.28+ no `PATH` | `opusic-sys`; sem ele o clippy morre sem dizer por quê |
| Node 22 + pnpm 10 | | só para subir o SFU local |
| Placa de vídeo | NVIDIA (NVENC), Intel (QuickSync) e AMD (AMF), se der: cada MFT tem as suas manias | encoder e decodificador na placa |

Código no WSL e Rust no Windows: [BUILD-WINDOWS.md](BUILD-WINDOWS.md).

### 3.2 Build, lint e testes (derivado)

```powershell
Set-Location "$(git rev-parse --show-toplevel)/native"
cargo clippy --workspace --exclude unkvoid-linux --exclude unkvoid-desktop --all-targets -- -D warnings
cargo test --workspace --exclude unkvoid-linux --exclude unkvoid-desktop
# Com hardware: o monitor duplicado, o encoder de verdade, o decodificador de verdade
cargo test -p capture -p media -- --ignored --nocapture
cargo run -p media --example encoder
```

No contêiner o que se provou foi a compilação (comprovado):

```bash
sudo apt-get install -y gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 && rustup target add x86_64-pc-windows-gnu
cd "$(git rev-parse --show-toplevel)/native" && cargo clippy --target x86_64-pc-windows-gnu -p media -p capture -p core-app -p clips -p storage -p unkvoid-windows --all-targets -- -D warnings
```

O alvo `gnu` confere todo `cfg(windows)`, mas não linka nem roda; o alvo MSVC precisa do SDK da
Microsoft, que o `cargo-xwin` baixa de um endereço bloqueado no contêiner.

### 3.3 O que mudou no Windows, e precisa de prova

Escrito pela leitura do código, compilado, **nunca rodado**:

| Mudança | Arquivo | Como provar |
|---|---|---|
| A ponte do encoder devolve a referência da vista de entrada (vazava uma por quadro; a memória da placa crescia a cada ponte refeita) | `media/src/windows.rs` (`cross`) | transmitir 10 min arrastando a borda de uma janela compartilhada; a "Memória dedicada da GPU" do Gerenciador de Tarefas não pode crescer sem parar |
| O MFT da placa é desligado (`MFShutdownObject`) no `Drop` | `media/src/windows.rs` | trocar a qualidade 10 vezes seguidas: o encoder continua "gpu" (barra da transmissão), sem cair para "cpu" |
| `MF_E_TRANSFORM_STREAM_CHANGE` no encoder aceita o tipo novo **e pede a mesma saída de novo na hora** (o MFT assíncrono não repete o evento) | `media/src/windows.rs` (`take_output`) | numa Intel (QuickSync): a transmissão abre com "gpu" e não cai para "cpu"; **e** a latência: rolar uma página e parar — quem assiste vê a página parada em < 0,5 s, e não só quando a tela mexe de novo (um quadro atrás para sempre era o defeito) |
| O quadro acima do teto de fps atravessa a ponte (sem codificar), e a imagem parada se repete ~100 ms depois que a tela para | `media/src/windows.rs`, `core/src/sharing.rs` | rolar uma página e parar: quem assiste vê a página parada no lugar certo em < 0,5 s |
| A ponte que caiu guarda a última imagem (a repetição da tela parada não fica preta) | `media/src/windows.rs` | difícil de forçar; olhar no log por "WAIT_TIMEOUT" do keyed mutex durante um jogo pesado |
| Desktop Duplication: ¼ de quadro de folga no ritmo, a imagem mais nova guardada mesmo quando é cedo, o cursor reposicionado depois de reabrir, e o carimbo do QPC | `capture/src/windows_duplication.rs`, `capture/src/windows.rs` | Windows 10, monitor de 60 Hz, 60 fps: a barra mostra ~60 fps (antes ~40); trocar a resolução do monitor no meio: o cursor continua no lugar |
| O som da tela segue a espera da imagem só fora de 40 ms de folga, desce em emendas de até 25 ms e larga a espera da imagem pausada ou fechada | `apps/windows/src/sound.rs`, `apps/windows/src/watching.rs` | passo 3 e 5 do roteiro, com fone: nenhum estalo nem picote no som do vídeo enquanto a rede oscila |
| A câmera continua sem existir no Windows: quem diz é a captura (`capture::captures_cameras`), não a tela | `apps/windows/src/bridge.rs` | o botão fica apagado, com o porquê no balão; nas configurações não aparece a seção de câmera |

### 3.4 Executar (PowerShell)

Variável de ambiente no PowerShell vale para todos os comandos seguintes da mesma janela: tire
uma antes de pôr a outra.

```powershell
Set-Location "$(git rev-parse --show-toplevel)/native"
$env:UNKVOID_SERVER = "http://127.0.0.1:8000"
cargo run -p unkvoid-windows
# o encoder do processador, para comparar
$env:UNKVOID_ENCODER = "cpu"; cargo run -p unkvoid-windows; Remove-Item Env:UNKVOID_ENCODER
# o decodificador do processador
$env:UNKVOID_DECODER = "cpu"; cargo run -p unkvoid-windows; Remove-Item Env:UNKVOID_DECODER
```

O teste vivo do decodificador, com alguém transmitindo numa sala:

```powershell
Set-Location "$(git rev-parse --show-toplevel)/native"
$env:UNKVOID_ROOM = "<código>"; cargo test -p unkvoid-windows a_live_screen -- --ignored --nocapture; Remove-Item Env:UNKVOID_ROOM
```

O mesmo `live_room.rs` roda no Windows (menos os da câmera e os do GOP, que são do Linux): a tela
capturada é o monitor principal, então ponha o `sync.mkv` do script em tela cheia num player com
som antes.

## 4. Roteiro manual

Duas máquinas (ou uma máquina e o `room` do exemplo), a mesma sala, com o app de cada lado. Em
cada passo, o esperado; se não for o que acontece, é bug — anote o horário e siga para a seção 5.

| # | Faça | Esperado |
|---|---|---|
| 1 | A cria uma sala; B entra pelo código | os dois se veem na lista, com o toque de entrada |
| 2 | A compartilha a tela (escolhendo monitor; no Wayland, o seletor do sistema abre) | o cartão "AO VIVO" aparece em B em < 2 s, com imagem em < 1 s depois dele; a barra de A mostra "gpu" numa máquina com placa |
| 3 | A abre um vídeo com som; B liga o som do cartão (ícone do alto-falante) | B ouve o vídeo, e a boca e o som batem (diferença que não se percebe, < 80 ms), sem estalo nem picote; A **não** ouve o próprio som de volta |
| 4 | A rola uma página e para | B vê a página parada no mesmo lugar em < 0,5 s |
| 5 | A troca a qualidade (720 → 1080) e o fps (60 → 30) com a transmissão no ar | a imagem de B muda sem sumir (parada < 1,5 s), e o som não sai de sincronia |
| 6 | A compartilha uma janela em vez do monitor; fecha a janela | B vê só a janela; ao fechar, A recebe o aviso "A janela que você compartilhava foi fechada" e a transmissão para |
| 7 | (Linux, com duas webcams) A escolhe a câmera em Configurações → Voz e vídeo → Câmera, e liga | um segundo cartão "CÂMERA" em B, 640x360, da webcam escolhida, com a tela continuando; trocar a escolha com ela ligada troca a imagem |
| 8 | A e B abrem o microfone e falam | cada um ouve o outro, sem eco com fone; o anel verde de quem fala acende |
| 9 | B põe a tela em tela cheia, sai, e troca de cartão | sem travar a janela; o vídeo de fundo pausa quando outro está em tela cheia |
| 10 | B se ensurdece e volta; B muta A por pessoa, se ensurdece e volta | depois de voltar, A continua mudo para B (o mudo por pessoa é de B) e o som da tela volta como estava |
| 11 | B se ensurdece por 15 min (ou deixa o som da tela mudo 15 min) e volta | B ouve na hora — antes ficava até 11 min sem som |
| 12 | C entra na sala logo depois de B (menos de 1 s) | C vê a tela e a câmera de A em ~1 s, como B |
| 13 | A para de compartilhar e compartilha de novo | o cartão some em B e volta com imagem em < 4 s |
| 14 | Num servidor: um moderador move A para outro canal de voz com a tela no ar | A entra sozinho no destino, sem o toque de saída; a transmissão para na origem; ninguém fica com cartão parado |
| 15 | Tire o cabo (ou o Wi-Fi) de A por 10 s e devolva, com a tela e a câmera ligadas | em B a imagem para; volta sozinha em ~3 s depois da rede, sem ninguém clicar, **a câmera também**; A continua transmitindo |
| 16 | O mesmo em B | a imagem de B volta sozinha em ~3 s |
| 17 | Linux: A compartilha com um jogo tocando e para, cinco vezes; depois mata o app (`kill -9`) com a tela no ar, abre de novo e compartilha | o som da máquina de A continua funcionando (antes o PulseAudio caía); depois do `kill -9`, o som da tela sobe de novo e o jogo volta à saída normal quando A para |
| 18 | Deixe A transmitindo 30 min | nenhuma parada; no Windows, a memória da placa estável |

## 5. Logs e como reportar

| Onde | Caminho |
|---|---|
| Windows | `%LOCALAPPDATA%\com.unkvoid.desktop\unkvoid-AAAA-MM-DD.log` |
| Linux | `~/.local/state/unkvoid/unkvoid-AAAA-MM-DD.log` (`$XDG_STATE_HOME/unkvoid/` se ela estiver definida); quem roda pelo terminal vê as mesmas linhas no stderr |
| SFU local | a saída do `node dist/server.js` |
| E2E | `$PASTA` do script (padrão `/tmp/unkvoid-ponta-a-ponta`) |

Os dois logs do app gravam a partir do nível `info`. As linhas que dizem onde parou:
`transmissão: números` (captura, encoder, envio, a cada 10 s), `broadcast: encoder de vídeo
aberto` (`gpu` ou `cpu`), `assistir: a imagem ficou parada`, `o servidor recebe a tela e nada
chega aqui`, `o servidor parou de responder`, `a câmera não voltou depois da queda`,
`o sink de uma transmissão que morreu ainda estava de pé`. Para mais detalhe: `RUST_LOG=debug`
antes do comando (no PowerShell, `$env:RUST_LOG = "debug"`).

Para reportar: o sistema e a versão, a placa de vídeo e o driver, o passo do roteiro, o horário,
o que aconteceu e o que se esperava, e o log do dia dos dois lados em volta do horário. Bug de
transmissão sem o log de quem transmitiu não tem como ser achado.
