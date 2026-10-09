# Verificar o app nativo numa máquina Windows e numa Linux de verdade

Para o próximo agente (ou pessoa) que vai compilar e provar o app Slint (`native/apps/windows`, o
mesmo crate nos dois sistemas) e o núcleo (`native/shared`) em hardware de verdade. Escrito em
09/10/2026 depois de uma rodada inteira num contêiner Ubuntu 24.04 sem placa de vídeo, sem
webcam e sem Windows — o relatório dela está em [relatorios/stratus.md](relatorios/stratus.md).

O que está aqui:

1. [o que já foi provado e o que só a máquina prova](#1-o-que-já-foi-provado-e-o-que-falta)
2. [Linux: pré-requisitos, build, testes e execução](#2-linux)
3. [Windows: pré-requisitos, build, testes e execução](#3-windows)
4. [o roteiro manual, igual nos dois sistemas, com o resultado esperado em cada passo](#4-roteiro-manual)
5. [onde ficam os logs e como reportar](#5-logs-e-como-reportar)

Comandos marcados **(comprovado)** rodaram no contêiner exatamente como estão. Os do Windows
marcados **(derivado)** saíram da compilação cruzada (`x86_64-pc-windows-gnu`) e da leitura do
código: ninguém os rodou num Windows nesta rodada.

## 1. O que já foi provado, e o que falta

| Peça | Provado no contêiner | Só a máquina de verdade prova |
|---|---|---|
| Empacotar e desempacotar H.264 (FU-A, STAP-A, marker, volta do número e do relógio) | testes do `media` | — |
| Perda, reenvio (NACK/RTX), pedido de quadro-chave (PLI), jitter buffer | testes do `media` e o E2E com `UNKVOID_LOSS=3` | perda real de internet (Wi-Fi, 4G) |
| Captura de tela no Linux (X11, `ximagesrc`) + x264 | E2E: 1280x720 a 30 fps | Wayland pelo portal (o seletor do sistema), encoders de placa (`nvh264enc`, `vah264enc`) |
| Decodificar no Linux (`avdec_h264` dentro do processo) | testes + o app no Xvfb | 1080p60 e 4K numa CPU de verdade |
| Som da tela no Linux (PulseAudio, sink combinado) | E2E com PulseAudio 16.1 | PipeWire (Ubuntu 24.04 e Debian 12 vêm com ele) |
| Câmera no Linux | E2E com `videotestsrc` no lugar da webcam | uma webcam USB de verdade (`v4l2src`) |
| Microfone | E2E (PCM empurrado pelo `speak`) | o `pulsesrc` do app com um microfone de verdade |
| Sincronia imagem/som | E2E: mediana de 23 a 68 ms (alvo < 80) | o ouvido e o olho, com o alto-falante e a placa de verdade |
| Queda de rede de 10 s | E2E: a imagem volta de 1 a 3 s depois da rede, dos dois lados | trocar de rede (Wi-Fi → cabo), reiniciar o roteador |
| Mover de canal | E2E com o `kick` assinado | pelo Laravel, com o botão do app |
| Windows inteiro (WGC, Desktop Duplication, Media Foundation, WASAPI) | só compila (clippy limpo no alvo Windows) | **tudo** — a seção 3 é o roteiro |

## 2. Linux

### 2.1 Pré-requisitos (comprovado em Ubuntu 24.04.5)

| O quê | Versão usada | Para quê |
|---|---|---|
| Rust | 1.97.0 (`rustup default stable`) + `clippy` | edição 2024 e `let`-chains |
| GStreamer | 1.24.2 (`libgstreamer1.0-dev`, `libgstreamer-plugins-base1.0-dev`, `gstreamer1.0-plugins-{base,good,bad,ugly}`, `gstreamer1.0-libav`, `gstreamer1.0-tools`, `gstreamer1.0-x`, `gstreamer1.0-pulseaudio`, `gstreamer1.0-pipewire`) | captura e encoder (dentro do processo), decodificador, som |
| Janela | `libfontconfig1-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libwayland-dev`, `libegl1`, `libgles2`, `libgl1-mesa-dri` | Slint (winit + femtovg) |
| Chaveiro | `libdbus-1-dev` | `shared/storage` (Secret Service) |
| Opus | `cmake` 3.28 (e `libopus-dev`) | o `opusic-sys` morre sem `cmake` com uma mensagem que não diz isso |
| Som | `pulseaudio-utils` (`pactl`, `pacat`); servidor PulseAudio 16.1 ou PipeWire | filtro do som da tela, saída e microfone |
| Para os testes sem tela | `xvfb`, `x11-utils`, `xdotool`, `imagemagick`, `ffmpeg` 6.1, `pulseaudio`, `iptables` | E2E e capturas de tela |
| SFU local | Node 22.22, pnpm 10.28 (mediasoup 3.26.0 baixa o worker pronto) | os testes contra o SFU |

```bash
sudo apt-get install -y --no-install-recommends \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libgstreamer-plugins-bad1.0-dev \
  gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly gstreamer1.0-libav gstreamer1.0-x gstreamer1.0-pulseaudio gstreamer1.0-pipewire \
  libdbus-1-dev libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libegl1 libgles2 libgl1-mesa-dri cmake pkg-config libopus-dev pulseaudio-utils \
  xvfb x11-utils xdotool imagemagick ffmpeg pulseaudio iptables
```

### 2.2 Build, lint e testes (comprovado)

```bash
cd native
cargo build --workspace --exclude unkvoid-desktop --all-targets
cargo clippy --workspace --exclude unkvoid-desktop --all-targets -- -D warnings
cargo test --workspace --exclude unkvoid-desktop
```

`unkvoid-desktop` é o Tauri legado: fora daqui de propósito. O `unkvoid-linux` (GTK) foi
aposentado pelo dono, mas ainda compila e entra no workspace.

Os testes que pedem GStreamer com plugins, PulseAudio ou SFU no ar ficam `#[ignore]`:

```bash
# GStreamer de verdade: encoder, keyframe por PLI, taxa no ar, relógio da captura, decodificador
cargo test -p capture -p media -- --ignored
# PulseAudio no ar (precisa de um servidor de som; num contêiner, `pulseaudio --start`)
cargo test -p capture --lib linux_audio -- --ignored --test-threads=1
```

`--test-threads=1` no do som: dois testes subindo o sink combinado ao mesmo tempo derrubam o
PulseAudio 16 (é bug dele, e é por isso que o app só sobe um).

### 2.3 De ponta a ponta contra o SFU (comprovado)

Um script monta tudo — Xvfb, PulseAudio com saída nula, um vídeo de clarão e bipe a cada
segundo tocando na tela e no som, o SFU do repo — e roda os seis cenários do
`native/shared/core/tests/live_room.rs` e a queda de rede de 10 s de cada lado:

```bash
sudo native/shared/core/tests/ponta-a-ponta.sh          # como root: a queda de rede usa iptables
SEM_REDE=1 native/shared/core/tests/ponta-a-ponta.sh    # sem root, sem a queda de rede
```

Cada cenário imprime o que mediu; o esperado (o que saiu no contêiner, sem placa):

| Cenário | Esperado |
|---|---|
| `a_late_viewer_sees_and_hears_the_screen_in_sync` | primeira imagem < 3 s depois de entrar (saiu ~1 s), ≥ 25 imagens/s, nenhuma parada > 500 ms, 1280x720, desvio A/V mediano < 80 ms (saiu 23–68 ms) |
| o mesmo com `UNKVOID_LOSS=3` | o mesmo; a espera da imagem cresce e o som acompanha (saiu 23–59 ms) |
| `the_microphone_reaches_the_room` | > 50 blocos de voz em 3 s (saiu 133) |
| `a_camera_is_watched_beside_the_screen` | ≥ 60 imagens da câmera em 5 s, 640x360, a tela continua |
| `stopping_and_sharing_again_brings_the_picture_back` | a imagem da tela nova em < 4 s (saiu ~1 s) |
| `a_quality_change_keeps_the_clock_and_the_picture` | 12–18 imagens/s depois de pedir 15, parada < 1,5 s (saiu 106 ms), relógio do RTP descompassado < 150 ms (saiu 31 ms) |
| `a_moved_person_leaves_for_good_and_shares_in_the_new_room` | o aviso `moved` com o destino, a tela sai da origem e não volta sozinha, e transmite no destino |
| queda de rede de quem transmite / de quem assiste | a imagem volta sozinha de 1 a 3 s depois da rede (o script exige imagem em todo segundo a partir de 6 s depois da volta) |

Os testes um a um, com um SFU já no ar:

```bash
UNKVOID_SFU=ws://127.0.0.1:3000/sfu SFU_SECRET=<o do SFU> UNKVOID_CAMERA_SOURCE="videotestsrc is-live=true pattern=ball" \
  cargo test -p core-app --test live_room -- --ignored --test-threads=1 --nocapture
```

E o medidor de sempre, um processo de cada lado (agora decodifica também no Linux):

```bash
cargo run -p core-app --example room -- ws://127.0.0.1:3000/sfu sala-de-teste share 60
cargo run -p core-app --example room -- ws://127.0.0.1:3000/sfu sala-de-teste watch 30
```

### 2.4 O app (comprovado no Xvfb)

```bash
cd native
UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid-windows                       # femtovg (OpenGL)
UNKVOID_SERVER=http://127.0.0.1:8000 SLINT_BACKEND=winit-software cargo run -p unkvoid-windows   # sem OpenGL
```

Sem o Laravel local, o app precisa de `/health` (o SFU) e `/api/config` (`{"sfu": "ws://…"}`)
no `UNKVOID_SERVER`: um servidorzinho que responda os dois basta para a sala por código. Num
Linux de verdade, prove também: `SLINT_BACKEND=winit-x11` numa sessão Wayland, e o portal
(`UNKVOID_CAPTURE=portal`).

Variáveis úteis: `UNKVOID_ENCODER=cpu` (pula o encoder da placa), `UNKVOID_CAPTURE=x11|portal`,
`UNKVOID_CAMERA_SOURCE=<origem do GStreamer>` (câmera sem webcam), `UNKVOID_LOSS=<%>` (perda de
propósito na chegada).

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
cd native
cargo clippy --workspace --exclude unkvoid-linux --exclude unkvoid-desktop --all-targets -- -D warnings
cargo test --workspace --exclude unkvoid-linux --exclude unkvoid-desktop
# Com hardware: o monitor duplicado, o encoder de verdade, o decodificador de verdade
cargo test -p capture -p media -- --ignored --nocapture
cargo run -p media --example encoder
```

No contêiner o que se provou foi a compilação: `cargo clippy --target x86_64-pc-windows-gnu -p media
-p capture -p core-app -p clips -p storage -p unkvoid-windows --all-targets -- -D warnings` limpo,
com o mingw-w64 (o alvo MSVC precisa do SDK da Microsoft, que o `cargo-xwin` baixa de um endereço
bloqueado no contêiner). O alvo `gnu` confere todo `cfg(windows)`, mas não linka nem roda.

### 3.3 O que mudou no Windows nesta rodada, e precisa de prova

Escrito pela leitura do código, compilado, **nunca rodado**:

| Mudança | Arquivo | Como provar |
|---|---|---|
| A ponte do encoder devolve a referência da vista de entrada (vazava uma por quadro; a memória da placa crescia a cada ponte refeita) | `media/src/windows.rs` (`cross`) | transmitir 10 min arrastando a borda de uma janela compartilhada; a "Memória dedicada da GPU" do Gerenciador de Tarefas não pode crescer sem parar |
| O MFT da placa é desligado (`MFShutdownObject`) no `Drop` | `media/src/windows.rs` | trocar a qualidade 10 vezes seguidas: o encoder continua "gpu" (barra da transmissão), sem cair para "cpu" |
| `MF_E_TRANSFORM_STREAM_CHANGE` no encoder aceita o tipo novo | `media/src/windows.rs` | numa Intel (QuickSync): a transmissão abre com "gpu" e não cai para "cpu" no primeiro segundo |
| O quadro acima do teto de fps atravessa a ponte (sem codificar), e a imagem parada se repete ~100 ms depois que a tela para | `media/src/windows.rs`, `core/src/sharing.rs` | rolar uma página e parar: quem assiste vê a página parada no lugar certo em < 0,5 s (antes podia ficar no meio da rolagem até a próxima mudança, ou 1 s atrás) |
| A ponte que caiu guarda a última imagem (a repetição da tela parada não fica preta) | `media/src/windows.rs` | difícil de forçar; olhar no log por "WAIT_TIMEOUT" do keyed mutex durante um jogo pesado |
| Desktop Duplication: ¼ de quadro de folga no ritmo, a imagem mais nova guardada mesmo quando é cedo, o cursor reposicionado depois de reabrir, e o carimbo do QPC | `capture/src/windows_duplication.rs`, `capture/src/windows.rs` | Windows 10, monitor de 60 Hz, 60 fps: a barra mostra ~60 fps (antes ~40); trocar a resolução do monitor no meio: o cursor continua no lugar |
| A câmera continua sem existir no Windows | `apps/windows/src/bridge.rs` | o botão fica apagado, com o porquê no balão |

### 3.4 Executar

```powershell
cd native
$env:UNKVOID_SERVER="http://127.0.0.1:8000"; cargo run -p unkvoid-windows
# o encoder do processador, para comparar
$env:UNKVOID_ENCODER="cpu"; cargo run -p unkvoid-windows
# o decodificador do processador
$env:UNKVOID_DECODER="cpu"; cargo run -p unkvoid-windows
```

O teste vivo do decodificador, com alguém transmitindo numa sala:
`UNKVOID_ROOM=<código> cargo test -p unkvoid-windows a_live_screen -- --ignored --nocapture`.

O mesmo `live_room.rs` roda no Windows (menos o da câmera): a tela capturada é o monitor
principal, então ponha o `sync.mkv` do script em tela cheia num player com som antes.

## 4. Roteiro manual

Duas máquinas (ou uma máquina e o `room` do exemplo), a mesma sala, com o app de cada lado. Em
cada passo, o esperado; se não for o que acontece, é bug — anote o horário e siga para a seção 5.

| # | Faça | Esperado |
|---|---|---|
| 1 | A cria uma sala; B entra pelo código | os dois se veem na lista, com o toque de entrada |
| 2 | A compartilha a tela (escolhendo monitor; no Wayland, o seletor do sistema abre) | o cartão "AO VIVO" aparece em B em < 2 s, com imagem em < 3 s; a barra de A mostra "gpu" numa máquina com placa |
| 3 | A abre um vídeo com som; B liga o som do cartão (ícone do alto-falante) | B ouve o vídeo, e a boca e o som batem (diferença que não se percebe, < 80 ms); A **não** ouve o próprio som de volta |
| 4 | A rola uma página e para | B vê a página parada no mesmo lugar em < 0,5 s |
| 5 | A troca a qualidade (720 → 1080) e o fps (60 → 30) com a transmissão no ar | a imagem de B muda sem sumir (parada < 1,5 s), e o som não sai de sincronia |
| 6 | A compartilha uma janela em vez do monitor; fecha a janela | B vê só a janela; ao fechar, A recebe o aviso "A janela que você compartilhava foi fechada" e a transmissão para |
| 7 | (Linux) A liga a câmera | um segundo cartão "CÂMERA" em B, 640x360, com a tela continuando |
| 8 | A e B abrem o microfone e falam | cada um ouve o outro, sem eco com fone; o anel verde de quem fala acende |
| 9 | B põe a tela em tela cheia, sai, e troca de cartão | sem travar a janela; o vídeo de fundo pausa quando outro está em tela cheia |
| 10 | B se ensurdece e volta; B muta A por pessoa, se ensurdece e volta | depois de voltar, A continua mudo para B (o mudo por pessoa é de B) e o som da tela volta como estava |
| 11 | A para de compartilhar e compartilha de novo | o cartão some em B e volta com imagem em < 4 s |
| 12 | Num servidor: um moderador move A para outro canal de voz com a tela no ar | A entra sozinho no destino, sem o toque de saída; a transmissão para na origem; ninguém fica com cartão parado |
| 13 | Tire o cabo (ou o Wi-Fi) de A por 10 s e devolva | em B a imagem para; volta sozinha em ~3 s depois da rede, sem ninguém clicar; A continua transmitindo |
| 14 | O mesmo em B | a imagem de B volta sozinha em ~3 s |
| 15 | Linux com PulseAudio (não PipeWire): A compartilha com um jogo tocando e para, cinco vezes | o som da máquina de A continua funcionando (antes o PulseAudio caía) |
| 16 | Deixe A transmitindo 30 min | nenhuma parada; no Windows, a memória da placa estável |

## 5. Logs e como reportar

| Onde | Caminho |
|---|---|
| Windows | `%LOCALAPPDATA%\com.unkvoid.desktop\unkvoid-AAAA-MM-DD.log` |
| Linux | `~/.local/state/unkvoid/` |
| SFU local | a saída do `node dist/server.js` |
| E2E | `$PASTA` do script (padrão `/tmp/unkvoid-ponta-a-ponta`) |

As linhas que dizem onde parou: `transmissão: números` (captura, encoder, envio, a cada 10 s),
`broadcast: encoder de vídeo aberto` (`gpu` ou `cpu`), `assistir: a imagem ficou parada`,
`o servidor recebe a tela e nada chega aqui`, `o servidor parou de responder`. Para mais
detalhe: `RUST_LOG=debug`.

Para reportar: o sistema e a versão, a placa de vídeo e o driver, o passo do roteiro, o horário,
o que aconteceu e o que se esperava, e o log do dia dos dois lados em volta do horário. Bug de
transmissão sem o log de quem transmitiu não tem como ser achado.
