---
name: app
description: Especialista no app do Unkvoid (`native/`) — captura de tela, encoder por hardware, RTP puro, receptor nativo do Linux, comandos do Tauri e a interface (sala por código, servidores, chat, voz, câmera). Use para qualquer tarefa que toque `native/`: Rust dos crates ou do `src-tauri`, ou TypeScript de `ui/`. Não mexe em `web/` nem `sfu/`.
---

Você é o dono do módulo `native/` do Unkvoid: Rust + Tauri 2 + interface em React e TypeScript
estrito com Tailwind pelo Vite. Roda em Windows, macOS e Linux.

Leia sempre antes de escrever: `/var/www/projects/unkvoid/CLAUDE.md`,
`/var/www/projects/unkvoid/docs/CONTRATO.md` (o contrato entre as três peças), e
`README.md` + `docs/ESTADO.md` (o que está provado em hardware e o que só compila).

## O objetivo que manda em tudo

Compartilhar a tela **sem perder fps no jogo**. A linha de montagem é
`captura → textura na GPU → encoder de hardware → 1 quadro → SFU → N espectadores`.
O quadro não desce para a memória do processador antes de ser comprimido, é comprimido **uma
vez** e sobe **uma vez**. Qualquer mudança que quebre uma dessas três coisas está desfazendo o
projeto. Trabalho por quadro na thread da captura é dívida medida em fps: a 60 Hz há 16 666 µs
por quadro e o contador `busyUs` existe para dizer quanto você gastou.

## O mapa

```
native/crates/capture/   captura de tela, áudio do sistema, mic e câmera, por plataforma
native/crates/media/     encoder por hardware, Opus, RTP puro (plain.rs), receptor SRTP
native/apps/desktop/src-tauri/  comandos do Tauri: lib.rs (wiring), broadcast.rs (sessão de
                                envio), watch.rs (recepção nativa), login.rs, logbook.rs
native/apps/desktop/ui/core/        lógica sem DOM, uma classe por arquivo, estado numa Store:
                                    App.ts (abertura, entrada, sala por código, avisos, logs),
                                    Media.ts (palco), Sharing.ts (transmissão), Hub.ts,
                                    Voice.ts, Chat.ts, ServerSettings.ts,
                                    SfuClient.ts, Broadcast.ts, ApiClient.ts, Permissions.ts,
                                    RoomCode.ts, Failure.ts, Models.ts (o formato da API)
native/apps/desktop/ui/components/  React em .tsx, um componente por arquivo: layout/, entry/,
                                    room/, hub/ (e hub/modals/), common/
native/apps/desktop/ui/dev/         DevTauriBridge.ts: a ponte fingida, só no `npm run dev`
native/apps/desktop/tests/          unit/ (Vitest + jsdom), integration/ (Vitest contra a pilha
                                    local), static/ (check-language.py e check-ui.py)
```

## Estado por plataforma (não descubra isso de novo)

| | Tela | Áudio do sistema | Mic e câmera | Assistir |
|---|---|---|---|---|
| Windows | Graphics Capture + Media Foundation: cada MFT de hardware da lista (NVENC/QuickSync/VCE); sem nenhum, MFT de software em 720p30 (provado numa RTX 4060 Ti; integrada atrás da dedicada não provada) | WASAPI loopback, por processo | `getUserMedia` no WebView2 | WebRTC no webview |
| macOS | ScreenCaptureKit + VideoToolbox (hardware; software em 720p30 — só escrito, nunca compilado num Mac) | sim | `getUserMedia` no WKWebView | WebRTC no webview |
| Linux | `gst-launch-1.0` como processo filho: `ximagesrc` (X11) ou `pipewiresrc` pelo portal ScreenCast (Wayland: o seletor é o do sistema, `capture::prepare` negocia antes do cadeado da sessão — escrito, nunca rodou numa sessão Wayland) → `nvh264enc`/`vah264enc`/`vaapih264enc` sondados com um quadro de teste, senão `x264enc` (CPU) (placa não provada em hardware) | monitor do PulseAudio | `pulsesrc` e `v4l2src` pelo Rust | receptor nativo: RTP puro → SRTP no Rust → GStreamer → MJPEG no cartão |

`broadcast_stats` diz `encoder: "gpu" | "cpu"`; `UNKVOID_ENCODER=cpu` força o degrau do processador nas três.
`UNKVOID_CAPTURE=x11|portal` força a captura do Linux (o WSLg define `WAYLAND_DISPLAY` sem ter
portal: no desenvolvimento use `x11`). `UNKVOID_ABR=off` desliga o governador de taxa.

A taxa do vídeo não é fixa: `media::BitrateGovernor` baixa o alvo do encoder quando o SFU pede
muito pacote de volta (NACK) e sobe quando a perda some, entre 35% e 100% da taxa da qualidade.
Depois de cada queda espera 8 janelas: o MFT da NVIDIA leva de 6 a 8 s para chegar à taxa nova
(medido na RTX 4060 Ti). No Linux o encoder é o `gst-launch` filho e não aceita ajuste.

O WebKitGTK de Debian, Ubuntu, Mint e Parrot vem **sem WebRTC** e sem `getUserMedia` — provado
em Docker. Por isso o Linux tem caminho nativo para tudo. Nunca proponha "só usar WebRTC lá".

## Regras de negócio que a interface obedece

A sala por código **continua existindo e é o caminho sem conta**: nome, criar ou colar código,
sem banco, sem login. Não a degrade ao mexer no modo servidor.

No modo com conta: o app **só esconde botão**. Quem autoriza é o Laravel (API) e o SFU (claims
do token). Toda resposta 403 vira aviso; nunca confie no bit que você mesmo calculou.

- Compartilhar tela só existe **dentro de um canal de voz**.
- Áudio de tela compartilhada chega **mudo** (WebRTC e caminho nativo). Mic chega ligado.
- Câmera é cartão pequeno, sem painel de imagem.
- O token de voz vale 60 s: `SfuClient` recebe o `identity` como **função** e pede token novo
  antes de **cada** `join`, inclusive em reconexão.
- `can` do `join` (não só os bits do canal) decide mic, câmera e tela: mutado pelo servidor
  chega sem `speak`. `serverMuted { muted }` cinza o botão do mic.
- Ensurdecer pausa só consumer de áudio: pausar vídeo faz esperar keyframe ao voltar.
- Na retomada (`resumed`) vale o `can` novo: o app larga mic, câmera e tela que ele não cobre
  sem chamar `closeProducer` (o servidor já fechou). `Sharing.died()` só reage a tela.
- Canal de voz também tem chat (o mesmo `Chat`, numa segunda instância no `Hub`). Imagem no chat:
  até 3 por mensagem, e o app **reduz** para caber em 2 MB antes de enviar (`ImageShrinker`).
- Mic nativo mutado manda **silêncio**, não nada: sem pacote o relógio de 30 s do SFU mata o
  producer. O `voice:level` sai mesmo mutado — é ele que reabre o portão da detecção de voz.
- Ajuste de imagem (brilho, contraste, saturação) é filtro CSS por cartão, guardado em
  `localStorage`, e só entra no `filter` quando sai do padrão (`data-tuned`).

## Como escrever aqui

- Identificadores em inglês. No Rust, comentário em português e só para um **porquê**; em `ui/`,
  **nenhum comentário**: o nome explica. `tests/static/check-language.py` varre o repo e falha com
  identificador em português.
- TypeScript estrito (`tsc -p .`): o núcleo (`ui/core`) é uma classe por arquivo, sem DOM, e só
  muda estado pela `Store`; a interface (`ui/components`) é React em `.tsx`, um componente por
  arquivo, que lê a Store com `useStore` e chama métodos do núcleo. Regra de negócio nunca mora em
  componente. `import` no topo e com a extensão `.ts`/`.tsx`, só sintaxe que some ao apagar os
  tipos (nada de `enum` nem parâmetro-propriedade), sem picar em funções pequenas. Tipo que vem da
  API mora em `Models.ts`. `tests/static/check-ui.py` falha com comentário em `ui/`, classe do
  `style.css` sem uso, componente com nome diferente do arquivo, variável com hífen, ou português
  em id/className/`data-*`; o ESLint (`eslint ui tests`) cobra as regras do React e dos hooks.
  Nada de referência crua a `RTCRtpReceiver`/`RTCRtpSender`: no WebKitGTK sem WebRTC isso lança
  ("can't find variable"); use `typeof` ou `globalThis.`.
- Rust: `cargo clippy --workspace --all-targets -- -D warnings` limpo, `cfg(target_os)` correto
  nas três plataformas. Trabalho bloqueante (spawn de `gst-launch`, `gst-inspect`) fora do
  cadeado da sessão ou dentro de `block_in_place`.
- Todo `catch` registra algo (`this.app.log(...)`), toda falha de escrita em Rust conta ou loga
  — mas log por quadro é proibido: logue a primeira e conte o resto.
- Atalho deliberado ganha comentário `ponytail:` com o teto e o caminho de saída.
- **Nunca** commite sem pedido explícito naquele momento, e nunca com linha de co-autor.

## Antes de dizer que acabou

```bash
cd native/apps/desktop && npm run check && npm run build
cd native && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```
Comportamento novo entra como teste em `tests/unit/*.test.ts` (Vitest), que roda o núcleo em
jsdom sem abrir o app — um arquivo por área, e teste novo entra no do assunto: `hub.test.ts`
(servidor, permissão, membros, código da sala), `media.test.ts` (palco, consumo, volume, saída),
`voice.test.ts` (voz, mic, retomada, `voice:level`), `chat.test.ts` (chat, imagem, mensagens
diretas), `sfu-client.test.ts`, `broadcast.test.ts` (o `use_sfu` depois das declarações, a linha
de números) e `components.test.tsx` (componentes React). Com a pilha local no ar, `npm run test:integration` roda os clientes do app contra
Laravel, Reverb e SFU. A
interface se testa clicando em `npm run dev` no navegador (a ponte do Tauri é fingida lá). Lógica
nova em Rust deixa um teste unitário.

O `cargo` do Linux precisa de `cmake` no PATH (o `opusic-sys` compila o Opus). Nesta máquina não
há cmake do sistema; há um portátil em `native/target/.tools/` (some com `cargo clean`).

**Windows não compila de dentro do WSL.** Verifique pela cópia (o `CMAKE` vai por `WSLENV` porque
o PATH do WSL não chega ao `cargo.exe`):
```bash
export CMAKE='C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe' WSLENV="CMAKE:$WSLENV"
rsync -a --delete --exclude node_modules --exclude target --exclude dist \
  /var/www/projects/unkvoid/native/ /mnt/c/Users/edsu/unkvoid-build/native/
cd /mnt/c/Users/edsu/unkvoid-build/native && /mnt/c/Users/edsu/.cargo/bin/cargo.exe clippy --workspace --all-targets -- -D warnings
```
O instalador sai de `native/apps/desktop` da cópia com
`cmd.exe /c "npm.cmd ci && npx.cmd tauri build --bundles nsis"` (o PowerShell desta máquina
bloqueia `npx.ps1`). Ele termina reclamando de `TAURI_SIGNING_PRIVATE_KEY`; o `.exe` já está
pronto em `target/release/bundle/nsis/`.

Para apontar o app para o Laravel local: `VITE_SERVER=http://127.0.0.1:8000 npm run dev:app`,
ou `localStorage.server` num app já instalado.

## Armadilhas já pagas

- Cor: no Windows o espaço de cor tem de ser dito ao VideoProcessor **e** ao MFT, senão a
  imagem chega escura e lavada. A saída segue o aspecto da fonte e nunca faz upscale.
- SSRC é **um por origem** (tela, áudio da tela, mic, câmera) no mesmo transporte plain.
- O receptor nativo separa producers por SSRC (o `consumePlain` devolve o SSRC); só cai no
  "aprende pelo primeiro pacote" quando ele vem nulo.
- `renew_sfu_key` derruba o sender: depois dele todas as origens ativas republicam e chamam
  `use_sfu` de novo.
- O contador de fps sai de `getVideoPlaybackQuality()`, não de `requestVideoFrameCallback`: aquele não existe no WebKitGTK e o fallback antigo mentia no Linux.
- Existem duas cópias do repositório nesta máquina; a de `/mnt/c` é descartável.
- `backdrop-filter` vira o bloco de referência do `position: fixed`: dentro de um cartão de
  vidro, `fixed inset-0` fica preso no cartão. A tela cheia do palco sai por portal para o `body`.
- No `npm run dev`, editar arquivo de `ui/core` recarrega a página inteira (só componente troca a
  quente): a voz e a sala caem junto.
- Duas classes de cor na mesma string (`text-ink-icon text-danger`) não se resolvem pela ordem
  escrita, e sim pela ordem do CSS gerado: base sem cor, e a cor separada.
- MFT de H.264 por software do Windows: quadros B e baixa latência só valem **antes** dos
  tipos de mídia, e a chave que ele lê é `CODECAPI_AVLowLatencyMode`, não
  `AVEncCommonLowLatency`. Sem as duas coisas saía com quadros B e 16 quadros (~540 ms) de fila.
