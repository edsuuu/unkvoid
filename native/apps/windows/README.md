# Unkvoid no Windows e no Linux

Rust + Slint, **um crate para o Windows e o Linux**. O núcleo entra **como crate**, direto — sem
ABI C e sem JSON no meio. É o app publicado no site e o pacote da Microsoft Store; no Linux, o
`.deb`. O que é de um sistema só fica atrás de `cfg(target_os)`: o WASAPI e o instalador NSIS no
Windows; o PulseAudio, o portal do Wayland e o `.deb` no Linux. O desenho é o mesmo nos dois, e
dá para abrir a janela no macOS para conferi-lo.

## Rodar

```bash
cargo run -p unkvoid-windows                                          # contra https://unkvoid.com
UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid-windows     # contra a pilha local
cargo clippy -p unkvoid-windows --all-targets -- -D warnings
cargo test -p unkvoid-windows
```

O endereço do SFU não é variável: vem do `GET /api/config`. O `opusic-sys` compila C e pede o
`cmake` no PATH (o do Visual Studio Build Tools serve; o `build-installer.ps1` o acha sozinho).
Instalador e pacote da Store: [docs/BUILD-WINDOWS.md](../../../docs/BUILD-WINDOWS.md).

Para diagnóstico:

| Variável | O que faz |
|---|---|
| `UNKVOID_ENCODER=cpu` | transmite pelo encoder de software (720p30), sem a placa |
| `UNKVOID_DECODER=cpu` | assiste decodificando na CPU, sem o DXVA |
| `UNKVOID_QUALITY_VS_SPEED=0..100` | o preset do encoder da placa (padrão 50) |
| `UNKVOID_DUPLICATION=on` | captura o monitor pelo Desktop Duplication também no Windows 11 |
| `UNKVOID_LOSS=3` | joga fora 3% do RTP que chega, para provar a recuperação |
| `UNKVOID_ABR=off` | a taxa do vídeo fica fixa, sem o governador |

Teste vivo, sem janela, com alguém transmitindo numa sala:
`UNKVOID_ROOM=<código> cargo test -p unkvoid-windows a_live_screen -- --ignored --nocapture`.

### Linux

A janela é do `winit`, em X11 ou Wayland (o `SLINT_BACKEND=winit-x11` força o X11 numa sessão
Wayland), desenhada pelo `femtovg` em OpenGL. Máquina sem OpenGL nenhum abre com
`SLINT_BACKEND=winit-software`. Para compilar: `libfontconfig1-dev`, `libxkbcommon-dev`,
`libwayland-dev`, `libdbus-1-dev`, `libgstreamer1.0-dev`, `libgstreamer-plugins-base1.0-dev`,
`cmake` e `pkg-config`. Para rodar: o GStreamer com os
plugins da captura e do decoder, e o `pactl`/`pacat` (`pulseaudio-utils`), que também falam
com o `pipewire-pulse` — a lista inteira está no `Depends` do `build-deb.sh`.

**Numa máquina sem Linux**, o `Dockerfile` desta pasta compila, testa e **abre** o app sob um
`xvfb`, com o OpenGL por software do Mesa:

```bash
cd native && docker build -f apps/windows/Dockerfile -t unkvoid-windows . && docker run --rm --cpus=2 unkvoid-windows
```

Para iterar sem recompilar o mundo a cada vez, monte o código e um volume para o `target`:

```bash
cd native && docker run --rm --cpus=2 -e CARGO_BUILD_JOBS=2 \
    -v "$PWD/shared:/app/shared" -v "$PWD/apps/windows:/app/apps/windows" -v unkvoid-windows-target:/app/target \
    unkvoid-windows bash -c 'cargo test -p unkvoid-windows && xvfb-run -a cargo run -p unkvoid-windows'
```

As cinco telas sem janela nenhuma saem do `vitrine` (`cargo run -p unkvoid-windows --example
vitrine -- <pasta>`), em BMP; e a janela de verdade sob o `xvfb` se fotografa com
`xwd -root -silent | convert xwd:- captura.png`.

O `.deb` sai de `./build-deb.sh` (Debian 12 num contêiner, sem GTK nenhum).

## Onde vai cada coisa

| Arquivo | O quê |
|---|---|
| `src/main.rs` | abre a janela, o log do dia, a instância única e entrega o laço ao Slint |
| `src/bridge.rs` | clique → Tokio → volta para a janela; traduz cada motivo do núcleo em frase. **Sem regra de negócio** |
| `src/watching.rs` | assistir: uma thread por tela, cada quadro guardado comprimido até a hora dele (`Playout`), decodificado na placa e escrito em RGBA direto na imagem; o som vai para o `sound.rs` |
| `src/sound.rs` | a saída e o microfone: pelo WASAPI no Windows, reabertos sozinhos quando o aparelho some ou o padrão muda; no Linux, um `pacat` alimentado pela mesma mistura e o `pulsesrc` do `shared/capture` |
| `src/devices.rs` | a lista de microfones e saídas: WASAPI no Windows, `pactl` no Linux |
| `ui/fonts/` | a Archivo (OFL), embutida e importada no `app.slint`: vale para a entrada e a sala por código (`Skin.sans`) |
| `Dockerfile`, `Dockerfile.deb`, `build-deb.sh` | o contêiner que compila, testa e abre o app no Linux, e o `.deb` |
| `src/stage.rs` | os cartões do palco, do jeito que a janela desenha |
| `src/frame.rs`, `ui/frame.slint` | a moldura da janela, com o botão de atualização ao lado do minimizar |
| `src/logbook.rs` | o log do dia e o envio das linhas com `ERROR` para o site |
| `src/sharing.rs` | testes vivos da captura e do encoder, sem sala |
| `src/clips/` | os Clips (replay instantâneo): bandeja, atalhos, galeria, player, aviso de "replay salvo", e o que muda quando o app roda como pacote da Store (`shell.rs`) |
| `ui/*.slint` | as telas: `entry`, `hub`, `room`, `voice`, `stage`, `share`, `settings`, `clips`, e o vocabulário do desenho em `skin` e `widgets` |
| `build-installer.ps1`, `installer.nsi` | o instalador do site |
| `build-msix.ps1`, `msix/AppxManifest.xml` | o pacote da Microsoft Store |
| `ui/skin.slint` | a paleta (`Skin`, o vidro da entrada e da sala por código; `Theme`, os tokens do Discord com os nomes do macOS), as medidas e os traçados dos ícones |
| `ui/widgets.slint` | o vocabulário do desenho: vidro, campo, botão, avatar, linha de lista — e o do Discord: `DButton`, `FlatIcon`, `MenuItem`, `Switch`, `Tip` |
| `ui/state.slint` | o `Ui`: o que a tela mostra e o nome de cada clique |
| `ui/voice.slint`, `ui/stage.slint` | a chamada: a grade 16:9 (pessoas e transmissões), o foco, a barra de botões redondos |
| `ui/userbar.slint` | o painel de voz e a barra de baixo: nome, microfone, áudio e a setinha de cada um |
| `ui/settings.slint`, `ui/server.slint` | as configurações em tela cheia (usuário e servidor), os modais e o de apelido |
| `examples/vitrine.rs` | desenha cada tela num BMP, sem janela: `cargo run --example vitrine -- <pasta>` |

O desenho de hoje é a réplica do Discord com os tokens do `Theme` (`ui/skin.slint`), os
mesmos nomes do `Theme.swift` do macOS. O vidro antigo (`Skin`) fica só na entrada e na sala
por código. `cargo run --example vitrine -- <pasta>` desenha cada tela num BMP, sem janela.

**Este crate é a interface do Windows e do Linux** (decisão de 09/10/2026; o GTK foi
aposentado). As telas (`ui/`, `bridge.rs`, `stage.rs`) não têm `cfg(target_os)`: o que é de
sistema (som, aparelhos, janela, empacotamento) fica atrás das mesmas assinaturas em
`sound.rs`, `devices.rs` e `watching.rs`.

## O desenho

A referência é o React em `apps/desktop/ui/`: os `.tsx` para comportamento e o `ui/style.css`
para os valores exatos; `ui/skin.slint` é a tradução dele. O `backdrop-filter` não tem par no
Slint (sobra o fundo translúcido sobre o halo violeta). A Archivo vai embutida (`ui/fonts/`) e
só vale onde o `Skin.sans` a pede; a réplica do Discord usa a fonte do sistema.

## Cuidados

- **A tela não espera rede.** O Slint desenha numa thread só; toda ida ao servidor sai por
  `Bridge::spawn` (Tokio) e volta por `upgrade_in_event_loop`.
- **Nada pesado na thread da janela**: decodificar, converter e soltar o que assiste (`Watch`)
  acontece fora dela.
- **Erro na tela não mostra caminho, URL nem status.** O núcleo devolve o motivo e o
  `bridge.rs` escreve a frase; o erro de validação do Laravel pinta o campo e escreve embaixo
  dele.
- **O Slint não recorta string**: a inicial do avatar e a hora da mensagem são cortadas no Rust.
- **O que o Linux e o macOS também precisariam não mora aqui**: é `shared/core`.
