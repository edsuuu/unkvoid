# Unkvoid no Linux

Rust + GTK4 (`gtk-rs`). **Não passa pela ABI C** — usa o `shared/core` como crate, direto.

## Rodar

```bash
cd native/apps/linux && cargo run
# apontando para o Laravel local
UNKVOID_SERVER=http://127.0.0.1:8000 cargo run
```

Precisa do GTK4 no sistema (`libgtk-4-dev` no Debian/Ubuntu). **Não compila em macOS nem em
Windows**: o `gdk-pixbuf-sys` procura a biblioteca pelo `pkg-config` e para ali.

Numa máquina sem GTK4, o `Dockerfile` desta pasta compila, testa e **abre** o app numa tela
que ninguém vê:

```bash
cd native && docker build -f apps/linux/Dockerfile -t unkvoid-linux . && docker run --rm unkvoid-linux
```

## Onde vai cada coisa

| Arquivo | O quê |
|---|---|
| `src/main.rs` | a janela, a pilha das cinco telas e o laço que consome a fila do `Bridge` |
| `src/bridge.rs` | clique → `core_app`; o trabalho vai para o Tokio e volta como `Update` |
| `src/components.rs` | os pedaços que se repetem: cartão, campo, lista, crachá |
| `src/screens/` | uma tela por arquivo: `entry`, `hub`, `room`, `offline`, `updating` |
| `src/user_bar.rs` | a barra de baixo: nome, microfone, som, e a setinha de cada aparelho |
| `src/sending.rs` | captura → `PlainSender`: tela, som da tela (sem o app de chamada — `shared/capture/src/linux_audio.rs`), microfone e câmera |
| `src/watching.rs` | `PlainReceiver` → `gst-launch` → pixels crus para a janela desenhar |
| `src/streaming.rs` | o que liga os dois à sinalização: `producePlain` e `consumePlain` |
| `src/devices.rs` | que microfone escutar e por onde sair o som, pelo `pactl` |
| `src/style.css` | a cor e o formato. Cor nunca no Rust |

Não há `src/platform/` ainda: nada de bandeja, notificação nem portal por enquanto.

## A regra desta pasta

**Nenhuma regra de negócio aqui.** O que o `bridge.rs` faz é traduzir clique em chamada e
resposta em `Update`. Quem decide se o código vale, onde se cai ao sair de uma sala, o que um
evento faz com a lista de quem está dentro e o que a pessoa pode fazer é o `shared/core`.

A única tradução que mora aqui é a **frase**: o núcleo devolve um motivo (`Failure`,
`EntryRefusal`) e `sentence()`/`refused()` escrevem o português. Motivo novo no núcleo para de
compilar aqui, que é o ponto.

## O que é do Linux, e não do núcleo

- Captura pelo portal do XDG no Wayland (ver `shared/capture/src/linux.rs`)
- Bandeja e notificação pelo D-Bus
- O caminho sem WebRTC: o `receiver.rs` do `shared/media` já é isto

## Como a mídia anda aqui

Não há segundo encoder: o H.264 sai do GStreamer do lado da captura (`shared/capture`) e o
`PlainSender` só o empacota. Do outro lado, o `PlainReceiver` abre o SRTP e um `gst-launch`
por transmissão decodifica — vídeo em RGB cru pela saída padrão, som direto no `pulsesink`.
A janela pega o quadro mais novo trinta vezes por segundo e o desenha como textura; quadro
atrasado é largado, e não enfileirado.

Microfone e câmera **não** usam `cpal` nem `nokhwa`: o `shared/capture` já os captura por
`pulsesrc` e `v4l2src`, com a mesma forma da tela.

## O que ainda não existe

- **Os modais de configuração** (servidor, canal, cargo, membro), as DMs e os amigos.
- **Chat por imagem.**
- **Escolher o que compartilhar**: vai sempre o monitor principal em 1080p60. A lista de
  monitores e janelas do `shared/capture` ainda não tem tela.
- **Nível de voz e detecção de voz**: o mic abre e fecha no botão, e não sozinho.

## A vantagem desta pasta

Sendo Rust, ela usa o `core` sem ponte, sem JSON no meio e sem liberar ponteiro à mão. É a
primeira onde vale provar um comportamento novo do núcleo — o que funcionar aqui atravessa
a ABI depois.
