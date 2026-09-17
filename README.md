# Unkvoid

Compartilhar a tela com quem você mandar o código, sem perder fps no jogo.

Você abre o app, escreve seu nome e clica em **Criar uma sala**. Sai um código de 12
caracteres. Manda o código para quem quiser; quem cola o código entra e vê a tela de quem
estiver transmitindo. O código **é** a sala: não existe em banco nenhum e some quando o
último sai.

## Por que um app, e não o navegador

No navegador o encoder de vídeo é da CPU. Transmitindo 1080p60 enquanto se joga, a CPU
disputa com o jogo e a transmissão cai para 1 fps ou trava — que é o problema que este
projeto existe para resolver. Aqui o caminho é outro:

```
captura → buffer de GPU → encoder da placa de vídeo → 1 quadro → SFU → N espectadores
```

O quadro nunca passa pela CPU antes de ser codificado, é codificado **uma vez**, e sobe
**uma vez** para o servidor, que replica. O upload de quem transmite não cresce com a
plateia. O encoder roda em tempo real e sem B-frames, que comprimem melhor mas exigem
reordenar quadros — latência que uma chamada não paga.

De quebra: sem barra do Chrome por cima, e o áudio do sistema entra junto.

## As três peças

| | O quê | Onde |
|---|---|---|
| `native/` | o app: captura, encoder, interface (Rust + Tauri) | na máquina de quem usa |
| `sfu/` | o relé de mídia (Node + mediasoup) | na VPS |
| `web/` | o site, as contas, os servidores com canais, o chat e a auditoria (Laravel) | na VPS |

A sala por código continua sem banco, mas exige conta: o Laravel só assina o token da
sala, o app fala com o SFU por um WebSocket só, e o `/health` existe para ele saber que o
servidor está de pé antes de deixar alguém entrar. O modo com conta (servidores, canais de texto e voz, cargos,
câmera) é o Laravel quem manda: ele decide quem entra em qual canal e assina o token
que o SFU confere. O contrato entre as três peças está em [docs/SERVIDORES.md](docs/SERVIDORES.md).

```
native/
  crates/capture/   captura de tela e áudio do sistema, por plataforma
  crates/media/     encoder por hardware, Opus, e o RTP puro que sobe para o SFU
  apps/desktop/     Tauri: os comandos e a interface
sfu/src/
  Http/             rota → Request → Controller → Resource
  Services/         Room, Peer, RoomRegistry, Signature, Webhook
web/
  app/Models/       Server, Channel, ServerRole, ServerMember, Message, ChannelAccess…
  app/Services/Sfu/ o cliente assinado que fala com o SFU (token, kick, mute, presença)
  routes/api.php    a API que o app usa; routes/channels.php, o Reverb
infra/nginx-unkvoid.conf   TLS, /sfu, /health, o Reverb em /app e /apps
```

## Estado por sistema

| | Captura de tela | Áudio do sistema | Encoder | Transmite? |
|---|---|---|---|---|
| macOS | ScreenCaptureKit | sim | VideoToolbox | sim |
| Windows | Graphics Capture | sim (WASAPI loopback, por processo) | Media Foundation (NVENC/QuickSync/VCE) | sim |
| Linux | GStreamer `ximagesrc` (X11) | monitor do PulseAudio/PipeWire | x264 (CPU, no GStreamer) | sim |

**Assistir** usa o WebRTC do webview. No Linux isso não existe: Debian, Ubuntu, Mint e
Parrot compilam o WebKitGTK **sem WebRTC**, e nenhum pacote do GStreamer muda isso
(provado em 11/09/2026 numa Debian 12 e numa Ubuntu 24.04 limpas, com `enable-webrtc`
ligado antes da página nascer: `typeof RTCPeerConnection` continua `undefined`). No
Linux o app cria sala e transmite (desde a 0.0.15) e assiste (desde a 0.0.16) por um
receptor nativo: o servidor manda a mídia por RTP puro (`consumePlain`), o Rust abre o
SRTP e o GStreamer decodifica e o quadro entra no cartão do app como MJPEG. `unkvoid-desktop --check` diz o que o
motor da janela desta máquina sabe fazer, e `unkvoid-desktop --check-capture` prova a
captura e o encoder em três segundos, sem abrir janela.

## Como buildar

Precisa em qualquer sistema: **Rust** (rustup, toolchain padrão) e **Node 22+**.

```bash
cd native/apps/desktop
npm ci
npx tauri build
```

O instalador sai em `native/target/release/bundle/`. **Não cross-compila**: cada
instalador só sai no seu próprio sistema.

### Windows

Além de Rust e Node, precisa do **Visual Studio Build Tools** com a carga "Desenvolvimento
para desktop com C++". O WiX o Tauri baixa sozinho.

```powershell
cd unkvoid\native\apps\desktop
npm ci
npx tauri build
```

Sai em `native\target\release\bundle\msi\Unkvoid_<versão>_x64_en-US.msi`.

Para o `.msi` sair assinado — **sem assinatura ninguém se atualiza sozinho**:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $HOME\.tauri\unkvoid.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
```

A chave pública já está no `tauri.conf.json`; a privada é `~/.tauri/unkvoid.key` e precisa
ser copiada para a máquina que gera o build.

### macOS

Precisa do Xcode Command Line Tools (mínimo 13.0, já configurado no
`native/.cargo/config.toml`).

```bash
cd native/apps/desktop
npx tauri build --bundles app dmg
```

Na primeira execução o sistema pede permissão de **Gravação de Tela**
(Ajustes → Privacidade e Segurança). A permissão vai para o app que *lançou* o processo —
rodando pelo terminal, é o terminal que aparece na lista.

### Linux

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev build-essential
cd native/apps/desktop && npm ci && npx tauri build
```

A captura é o `gst-launch-1.0` como processo (`ximagesrc` → `x264enc`, som pelo
`pulsesrc` no monitor da saída): precisa de `gstreamer1.0-tools` e dos plugins
good/ugly, que o `.deb` já exige. Só X11: em sessão Wayland pura a lista de telas sai
vazia (o caminho é `pipewiresrc` via portal). Sem lista de janelas ainda.

### Publicar uma release

Não há GitHub Actions — o build é feito na máquina de quem tem o sistema, e a release é
montada localmente com o `gh` autenticado:

```bash
cd native/apps/desktop
node release.mjs --dry-run   # mostra o que achou
node release.mjs             # cria/atualiza a release e o latest.json
```

O script lê o `latest.json` já publicado e mescla, então subir o Windows depois do macOS
não deixa os Macs sem para onde atualizar. **A release não pode ser marcada como
pré-lançamento**: o endpoint do auto-update é `/releases/latest/download/latest.json`, e o
"latest" do GitHub ignora pré-lançamentos — a URL responde 404 e ninguém atualiza.

## O servidor

Uma VPS, dois processos, nada mais:

```bash
cd sfu
pnpm install
pnpm run build
./deploy.sh vps      # rsync + pm2 restart + confere o /health
```

| Porta | Protocolo | Para quê |
|---|---|---|
| 443 | TCP | nginx: TLS para `wss://…/sfu` e `/health` |
| 3000 | TCP | o SFU, só em 127.0.0.1 (atrás do nginx) |
| 40000-40006 | UDP | WebRTC de quem assiste — uma porta por worker |
| 41000-41447 | UDP | RTP puro de quem transmite pelo app (a regra do firewall abre 41000-42000) |

As portas UDP precisam aceitar entrada não solicitada: o mediasoup é ICE Lite, só
responde e nunca inicia.

A sala é anônima por construção, então nada prova quem entra. O que impede varrer códigos
é o teto de conexões novas por IP (`SFU_CONNECTIONS_PER_MINUTE`, padrão 20) — e é por isso
que o nginx precisa mandar o `X-Forwarded-For`.

## O que verifica o quê

```bash
cd sfu && pnpm run build
SFU_CONNECTIONS_PER_MINUTE=200 node dist/server.js &   # o check abre uma dúzia de sockets
pnpm run check                                        # o contrato inteiro, num servidor de verdade

cd native/apps/desktop && npm run check               # tsc, ESLint, Vitest (tests/unit) e os checks de tests/static
cd native && cargo test --workspace                   # Opus em blocos de 20 ms, pacotização, SRTP
cargo run -p media --example plain -- <ws> <sala>     # o SFU confirmando que recebe o RTP
```

## Armadilhas já pagas

- **`npm run check` antes de qualquer commit no desktop.** Três vezes um script de
  substituição em bloco apagou um método inteiro do `app.js`. O sintoma é tela preta ou
  clique que não faz nada.
- **Teste no `harness.html`, não na janela do app.** A janela do Tauri não tem console: um
  erro de JS vira tela preta sem pista. O harness roda o mesmo bundle no navegador.
- **`use_sfu` só depois de declarar vídeo E áudio.** Ao contrário, o Rust manda RTP de um
  SSRC que o servidor ainda não conhece e ele descarta calado: a transmissão "funciona" e
  ninguém vê nada. O `tests/unit/broadcast.test.ts` guarda essa ordem.
- **`hidden` do Tailwind é classe, não atributo.** Alternar o atributo num elemento que
  tem a classe não faz nada.
- **`build.rs` tem `cargo:rerun-if-changed=../dist`.** Sem isso o cargo não recompila
  quando só o frontend muda, e o app sai com a interface antiga. Não remova.
- **O crate `capture` tem um módulo chamado `windows`.** Dentro dele, `windows::Win32::…`
  acha o módulo local em vez da crate da Microsoft. Precisa de `::windows::`.
- **Ponteiro COM não é `Send`.** O encoder atravessa uma vez para a thread da captura, e há
  um `unsafe impl Send` com a justificativa escrita: os objetos do D3D11 (com proteção
  multithread ligada) e o MFT assíncrono são livres de apartamento.

## Documentos

Tudo em [`docs/`](docs/):

| Arquivo | O que tem |
|---|---|
| `docs/SERVIDORES.md` | o contrato entre as três peças: API, token, eventos, comandos do Tauri |
| `docs/ESTADO.md` | o que só foi escrito sem rodar em hardware, o que falta e as perguntas abertas |
| `docs/DECISOES.md` | decisões de arquitetura e o porquê de cada uma |
| `docs/REDE.md` | o caminho da imagem, os ajustes de rede e o que mora fora do repositório |
| `docs/SEGURANCA.md` | o que está protegido, o que não está, e o que falta |
| `docs/AUTO-UPDATE.md` | assinatura, manifesto e os dois canais de atualização |
| `docs/SERVIDOR.md` | a VPS que existe: medições, firewall, repositório APT, o que desligar |
| `docs/INSTALAR-VPS.md` | levantar uma VPS do zero, em ordem, e migrar o e-mail sem perder mensagem |
| `docs/UDP.md` | quantas portas UDP a rede precisa abrir, e o que quebra calado quando aperta |
| `docs/BUILD-WINDOWS.md`, `docs/BUILD-MACOS.md` | como gerar instalador em cada sistema |
