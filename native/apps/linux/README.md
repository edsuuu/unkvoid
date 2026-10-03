# Unkvoid no Linux

Rust + GTK4 (`gtk-rs`). Usa o `shared/core` como crate, direto. É o pacote `unkvoid` do
repositório APT.

## Rodar

```bash
cargo run -p unkvoid-linux
UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid-linux   # contra a pilha local
UNKVOID_CAPTURE=x11 cargo run -p unkvoid-linux                     # força o ximagesrc (WSLg, sessão Wayland sem portal)
```

Precisa do GTK4 (`libgtk-4-dev`), do `libdbus-1-dev` (o chaveiro fala Secret Service), do
GStreamer com os plugins good, bad e ugly, do `pactl` e do `cmake` (o Opus compila em C). **Não
compila no Windows nem no macOS**: o `gdk-pixbuf-sys` procura a biblioteca pelo `pkg-config`.

Numa máquina sem GTK4, o `Dockerfile` desta pasta compila, testa e abre o app numa tela que
ninguém vê:

```bash
cd native && docker build -f apps/linux/Dockerfile -t unkvoid-linux . && docker run --rm unkvoid-linux
```

O `.deb`: [docs/BUILD-LINUX.md](../../../docs/BUILD-LINUX.md).

## Onde vai cada coisa

| Arquivo | O quê |
|---|---|
| `src/main.rs` | a janela, a pilha das telas, o log do dia (`~/.local/state/unkvoid`) e o laço que consome a fila do `Bridge` |
| `src/bridge.rs` | clique → `core_app`; o trabalho vai para o Tokio e volta como `Update`. Escreve a frase de cada motivo de erro |
| `src/screens/` | uma tela por arquivo: `entry`, `hub`, `room`, `offline`, `updating` |
| `src/components.rs`, `src/icons.rs` | os pedaços que se repetem e os ícones |
| `src/user_bar.rs` | a barra de baixo: nome, microfone, som e a setinha de cada aparelho |
| `src/share_picker.rs` | o seletor do que compartilhar: monitores e janelas, com a qualidade |
| `src/sending.rs`, `src/streaming.rs` | a ponte entre a captura e a sinalização do núcleo |
| `src/watching.rs` | assistir: um `gst-launch` por tela (RGB cru para a janela), alimentado no horário do `Playout`, e um por som direto na saída |
| `src/devices.rs` | que microfone escutar e por onde sair o som, pelo `pactl` |
| `src/style.css` | cor e formato. Cor nunca no Rust |
| `build-deb.sh`, `Dockerfile.deb` | o `.deb`, compilado num Debian 12 |

## Como a mídia anda aqui

Não há segundo encoder: o H.264 sai do GStreamer da captura (`shared/capture/src/linux.rs`) e o
núcleo só empacota. Microfone e câmera também vêm da captura (`pulsesrc`, `v4l2src`). Do outro
lado, o núcleo recebe, pede de novo o pacote perdido e remonta o quadro; aqui cada tela ganha um
`gst-launch` que decodifica (`avdec_h264`) e devolve RGB cru. A sala desenha a cada quadro do
monitor; sem esse relógio por 2 s (janela minimizada ou escondida), o vídeo é pausado no
servidor (`Room::set_away`).

## O que é do Linux, e não do núcleo

- a captura pelo portal do XDG no Wayland (`shared/capture/src/linux.rs`);
- os aparelhos de som pelo `pactl`;
- o chaveiro pelo Secret Service.

## O que ainda não existe

- as configurações do servidor: cargos, membros, banidos e auditoria (criar servidor e canal,
  o convite, os amigos e as mensagens diretas já existem);
- imagem no chat;
- nível de voz, detecção de voz e falar apertando: o microfone abre e fecha no botão;
- bandeja e notificação.

## A regra desta pasta

Nenhuma regra de negócio aqui. O `bridge.rs` traduz clique em chamada e resposta em `Update`;
quem decide é o `shared/core`. Sendo Rust, esta é a primeira pasta onde vale provar um
comportamento novo do núcleo.
