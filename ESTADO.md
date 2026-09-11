# Estado do projeto — o que falta e por quê

> Escrito em 08/09/2026, no fim de uma sessão longa, para quem pegar o trabalho depois.
> O [README.md](README.md) diz o que o projeto é e como buildar. Este arquivo diz **onde
> a coisa parou**, o que está provado, o que só compila, e o que ainda não existe.

## Atualização de 09/09/2026 (tarde) — o que foi corrigido e o que falta

> Escrito para continuar no Windows. O que está aqui é o estado real: o que foi
> provado em hardware, o que só compila, e o que não existe.

### O achado que muda a leitura do projeto

**Compartilhar tela do macOS nunca funcionou.** O VideoToolbox devolve H.264 em
AVCC, com prefixo de tamanho em cada NAL, e guarda SPS e PPS na descrição de
formato. O empacotador RTP só quebra Annex-B e só aprende os parameter sets se
eles passarem por ele. O quadro saía perfeito da GPU e chegava do outro lado como
um NAL de tipo 0, que nenhum decodificador exibe — com todo contador marcando
saúde. Corrigido e verificado em hardware: o exemplo `cargo run -p media --example
encoder` afirma que o keyframe sai com start code, SPS, PPS e IDR.

### Corrigido nesta sessão

| Onde | O quê |
|---|---|
| macOS | Bitstream AVCC → Annex-B com SPS/PPS |
| Transporte | Relógio RTP do vídeo derivava a cada quadro perdido |
| Transporte | Retry dormia até 512 ms na thread da captura |
| Windows | `METransformNeedInput` perdido pendurava a captura para sempre |
| Windows | Não capturava áudio nenhum; agora há WASAPI por processo |
| SFU | Sem heartbeat, socket meio aberto vazava sala até estourar a memória |
| SFU | Uma porta de RTP por worker: dois numa sala davam `no more available ports` |
| Cliente | `consume` usava variável não declarada, e a tela nunca era desenhada |
| Cliente | Ninguém escutava `closed`/`reconnected`: travava calado |
| Linux | WebKitGTK entrega WebRTC desligado; agora é ligado no Rust |
| Linux | Bandeja que falha derrubava o app inteiro na abertura |

### O estado por sistema

- **macOS** — transmite e assiste. Provado em hardware.
- **Windows** — compila para o alvo, mas **nada foi executado lá**. As correções
  do encoder e o áudio WASAPI foram escritos e type-checados, não testados. É o
  primeiro trabalho de quem pegar a máquina Windows.
- **Linux** — transmite (X11, via `gst-launch-1.0` + x264 na CPU, desde a 0.0.15) e
  assiste (0.0.16) pelo receptor nativo: RTP puro do servidor, SRTP aberto no Rust
  (`crates/media/src/receiver.rs`), GStreamer decodifica e entrega ao cartão do app como MJPEG
  (`src-tauri/src/watch.rs`). O WebKitGTK das distros segue sem WebRTC (ver README).
  Instala por `apt install unkvoid`.

### O que NÃO existe: servidores com salas

Foi pedido e **não foi feito**. O modelo continua sendo sala e mais nada: no SFU
há `Room`, `Peer` e `RoomRegistry`, sem nenhum conceito acima da sala, e a
interface ainda é "criar uma sala" ou colar um código.

Ficou por último de propósito: era a única frente que não consertava nada, e na
época o macOS não transmitia, o Windows congelava e duas pessoas não conseguiam
compartilhar na mesma sala. A fundação agora está diferente.

**O desenho foi decidido em 10/09/2026** e está abaixo. O que falta é execução,
não decisão.

#### O SFU não muda

Uma `Room` hoje é qualquer string que o cliente mandar, e o `RoomRegistry` cria
sob demanda. Isso já é um canal. O que muda é quem emite a string: hoje o cliente
sorteia em `room-code.js`, e passa a ser o Laravel.

O id do canal é ULID em minúsculas. São 26 caracteres de a-z0-9, então passa no
regex do `JoinRequest` sem tocar em uma linha. UUID não serve: 36 caracteres com
hífens, e o limite é 32.

#### As camadas

| Onde | O quê | Vida |
|---|---|---|
| Laravel | servidor, canal, membro, convite | banco, para sempre |
| Token assinado | canal, nome, dono, validade | um minuto |
| SFU | `Room`, `Peer`, mídia | enquanto tem gente dentro |

Isso responde a pergunta que estava aberta aqui. O servidor sobrevive a reinício
porque mora no banco. A sala continua não sobrevivendo, e não precisa — ninguém
quer um transport morto de volta depois de um deploy. São dois tempos de vida
diferentes, e é de propósito.

#### A entrada passa a ser autenticada

Hoje não existe auth para mover: o código da sala é a credencial, e o modelo é
"quem tem a string, entra". O Laravel decide quem entra em qual canal e assina um
token curto. O SFU só confere a assinatura com um segredo compartilhado, sem
chamada de rede no caminho do join.

```php
$claims = ['room' => $channel->id, 'name' => $user->name, 'owner' => $isOwner, 'exp' => time() + 60];
$body = rtrim(strtr(base64_encode(json_encode($claims)), '+/', '-_'), '=');

return $body . '.' . hash_hmac('sha256', $body, config('services.sfu.secret'));
```

Do lado do SFU, no lugar do `installId()`: comparar o HMAC com `timingSafeEqual`,
conferir o `exp` e ler os claims. Sem biblioteca de JWT em nenhum dos dois lados.

Sai do SFU com isso: o regex do código, `installId()`, `ownerInstallId`, o Set
`banned`, `isOwner` e o ramo de dono do `PeerController`. Dono e banimento sobem
para o servidor, que é onde as pessoas esperam que morem. A validade curta do
token é o que faz o ban valer: quem foi expulso não consegue token novo.

De brinde conserta o modelo de segurança. Hoje quem editar o app manda o
`installId` que quiser e vira dono de sala alheia.

#### A única mecânica nova

Presença fora da sala. Mostrar quem está no canal antes de entrar não existe
hoje, e não dá para deduzir: só se sabe entrando. O SFU já conta isso no
`/health`, então a versão barata é expor a contagem por canal ali e o Laravel ler
com cache de poucos segundos. Sem pub/sub e sem um segundo WebSocket.

#### O trabalho real é o cliente

O `room-code.js` inteiro sai, e com ele a tela de criar ou colar código. Entram
barra lateral de servidores, lista de canais e quem está em cada um. Somando a
tela de login, que também não existe, é o `app.js` que leva a pancada. O Laravel
é CRUD de um fim de semana; o SFU é uma tarde.

#### O que não construir

Chat, cargos, categorias, threads, emoji. Servidor, canal, membro, convite, e
dois papéis: dono e membro. O resto é imitar o Discord, e não é o que faz uma
chamada funcionar.

### Publicação, hoje

- **Auto-update**: ver [AUTO-UPDATE.md](AUTO-UPDATE.md). A chave privada está em
  `~/.tauri/unkvoid.key` no Mac e bate com a pública do app — ninguém precisa
  reinstalar.
- **Linux**: `sudo apt install unkvoid`, repositório em
  <https://discord.unkvoid.com/apt/>, índice refeito a cada `make build-vps`.
- **macOS e Windows**: `.github/workflows/release.yml`, disparado por tag `v*`.
  Falta cadastrar o segredo `TAURI_SIGNING_PRIVATE_KEY` no GitHub.

### Pendências conhecidas

1. **Rodar no Windows.** Nada do que foi escrito para lá foi executado.
2. **Contador de fps mente no Linux.** Sem `requestVideoFrameCallback`, o
   fallback conta eventos a 4/s e grava esse número no log de diagnóstico.
3. **H.264 pode não ser anunciado no Linux** se o GStreamer da distro exigir
   encoder além de decoder. Sintoma: tela preta sem erro. O log `device.ready`
   mostra os codecs.
4. **Falta `BUILD-LINUX.md`.** O README manda instalar quatro pacotes para
   compilar e faltam cinco, inclusive o `cmake`.
5. **Histórico do git** ainda tem 60 commits com linha de co-autor. A reescrita
   foi aprovada e não foi executada; exige force-push e re-clone.

## Atualização de 09/09/2026 — controles da transmissão

Foi publicada a correção no commit `90703f2`:

- O controle de volume agora fica no mesmo grupo dos botões **Focar** e
  **Tela cheia**, separado por transmissão.
- O botão **Tela cheia** tenta, nesta ordem, o card da transmissão, o
  elemento de vídeo e o fallback `webkitEnterFullscreen`, usado por alguns
  WebViews/Tauri.
- Quando o ambiente não suporta fullscreen ou a chamada falha, o erro é
  registrado e exibido na interface em vez de falhar silenciosamente.

Validações concluídas:

- `npm run check`
- `npm run build`
- `node --check native/apps/desktop/ui/app.js`
- `git diff --check`

Também foi gerado um novo build Windows com NSIS e MSI. Os arquivos foram
copiados para:

```text
C:\Users\edsu\Desktop\apps\Unkvoid_0.0.2_x64-setup.exe
C:\Users\edsu\Desktop\apps\Unkvoid_0.0.2_x64_pt-BR.msi
```

O build gera os instaladores normalmente, mas termina com aviso/erro ao tentar
criar artefatos do updater porque `TAURI_SIGNING_PRIVATE_KEY` ainda não está
configurada. Portanto, os instaladores podem ser testados manualmente; a
atualização automática só ficará completa depois que a chave privada de
assinatura for configurada e os artefatos `.sig`/`latest.json` forem
publicados.

## O objetivo, para não se perder

Compartilhar a tela **sem perder fps no jogo**. Todo o resto é consequência disso.

No navegador o encoder de vídeo roda na CPU, então o jogo e a compressão disputam o mesmo
processador e a transmissão cai para 1 fps. O app existe para usar o chip de codificação
da placa de vídeo. A linha de montagem é:

```
captura → textura na GPU → encoder de hardware → 1 quadro → SFU → N espectadores
```

O quadro nunca desce para a memória do processador antes de ser comprimido, é comprimido
**uma vez** e sobe **uma vez**. Qualquer mudança que quebre uma dessas três coisas está
desfazendo o projeto.

## Onde o trabalho está

Branch **`limpeza-so-o-sfu`**, empurrada para o GitHub. Dois commits sobre a `main`:

| Commit | O quê |
|---|---|
| `7c6dcc1` | Sai o Laravel, sai o P2P, e a reconexão passa a funcionar |
| `d3629f4` | Windows enfim transmite: encoder de hardware por Media Foundation |

A `main` **não** foi mexida. Para juntar: `git checkout main && git merge limpeza-so-o-sfu`.

## O que mudou nesta sessão

### 1. O Laravel morreu, e com ele o token

O app pedia ao Laravel um token para entrar na sala: 112 arquivos e ~7.900 linhas de PHP,
mais MySQL, Reverb e PHP-FPM, para servir dois endpoints. **Não existe mais endpoint
nenhum.** O código da sala é sorteado no cliente e o `join` recebe `{room, name}` direto
pelo WebSocket que já existia.

Tirar o token foi consequência, não economia: o SFU passaria a assinar o que ele mesmo
verifica. No lugar dele, quem prova identidade entre uma queda e a volta é uma
**`resumeKey`** secreta, devolvida só na resposta do `join`. O `peerId` não serve para
isso — a sala inteira o recebe no `peerJoined`, então aceitá-lo como identidade deixaria
qualquer um derrubar qualquer um. Há teste para esse caso no `sfu/check.mjs`.

Isso **consertou a reconexão, que nunca funcionou**: o `resume` casa pelo id do
participante, mas o emissor de token sorteava um `sub` novo a cada chamada e o cliente
pedia um token novo a cada reconexão. Quem caía nunca retomava — republicava tudo do zero
e ficava fantasma por 45 segundos.

O `throttle:20,1` do Laravel virou **teto de conexões novas por IP no próprio SFU**
(`SFU_CONNECTIONS_PER_MINUTE`, padrão 20). Com sala anônima nada prova quem entra; o que
impede varrer códigos é o custo de tentar. É por isso que o nginx precisa mandar o
`X-Forwarded-For` — sem ele, todo mundo vira o mesmo cliente `127.0.0.1`.

Também saiu, por não ter chamador nenhum: o caminho P2P em Rust (`peer.rs`, `PeerLink`,
quatro comandos do Tauri, as deps `webrtc` e `async-trait`), ~350 linhas de SFU da era
Discord (presença entre canais, moderação, mudo/surdo, relé de sinalização, `produce` por
WebRTC), os plugins `deep-link`/`opener`/`autostart`, o `settings.rs` (SQLite — nada usava,
o nome mora no `localStorage`) e o `.github/`.

### 2. O envio deixou de nascer uma task por quadro

Com um destino só, mandar o quadro virou síncrono na própria thread da captura, e o socket
UDP ficou não-bloqueante. Antes cada quadro nascia uma task do tokio, sessenta vezes por
segundo. Para vídeo ao vivo, perder um pacote custa menos do que perder fps.

### 3. O Windows ganhou a estação que faltava

Duas metades da mesma coisa faltavam:

- A captura pegava os quadros e **jogava os pixels fora** (`surface: None`), porque não
  havia encoder para recebê-los.
- O `PlatformEncoder` fora do macOS era um stub que recusava iniciar — então o botão
  Transmitir falhava antes de qualquer quadro existir.

Agora `capture::GpuSurface` no Windows é a textura do Direct3D **com o device e o contexto
que a criaram**. Os três andam juntos porque a textura pertence à rotação interna da
captura: só o device dela sabe lê-la, e ela vale apenas durante o callback.

O encoder (`native/crates/media/src/windows.rs`, 729 linhas) é o MFT de H.264 por
hardware — o mesmo caminho que NVENC (NVIDIA), QuickSync (Intel) e VCE (AMD) expõem ao
Windows. Mesmas decisões do VideoToolbox no macOS: tempo real, sem B-frames, keyframe a
cada 2 segundos.

**São dois devices do Direct3D, e não um.** O da captura nasce sem
`D3D11_CREATE_DEVICE_VIDEO_SUPPORT` (a crate `windows-capture` não expõe as flags), e sem
isso não há VideoProcessor nem gerente de device para o Media Foundation. A ponte entre os
dois é uma textura compartilhada com keyed mutex — e é nela que a conversão acontece: a
captura entrega **BGRA no tamanho nativo do monitor**, o encoder quer **NV12 no tamanho
escolhido**, e um blit do VideoProcessor faz as duas coisas na GPU. Escalar isso na CPU
devolveria exatamente o problema de fps que o app existe para resolver.

Encoder de hardware tem fila: os primeiros quadros entram sem nada sair. Por isso a saída é
uma `VecDeque` interna e `encode` devolve `NeedsMoreInput` enquanto ela está vazia, em vez
de fingir 1-entra-1-sai.

E o **fps deixou de ser 60 fixo**: a interface oferece 30 e 60, o número atravessa até a
captura (`MinimumUpdateIntervalSettings`) e até o encoder, e os dois concordam. Se
discordassem, o vídeo chegaria acelerado ou aos trancos. Metade dos quadros também custa
perto de metade da banda, então o bitrate acompanha.

## O que está provado, e o que só compila

**Provado, rodando de verdade:**

- **SFU**: `pnpm run check` contra um servidor local — o protocolo inteiro, incluindo a
  tentativa de retomar sessão com o id alheio e o ingest de RTP puro sendo consumido.
- **Teto por IP**: testado à mão, fecha com `1013` ao estourar.
- **Interface**: `npm run check` (ids, sorteio do código de sala, ordem da transmissão) e
  `vite build` fechando limpo.
- **Rust no Windows**: `cargo check --workspace --all-targets`, `cargo clippy -- -D
  warnings` e `cargo test --workspace` (11 testes) passam.
- **Deploy do SFU**: no ar em produção, `{"ok":true,...,"workers":[0,0,0,0]}`.

**Só compila — nunca executou:**

> ⚠️ **O encoder do Windows nunca rodou com captura real.** Ele type-checa, passa no
> clippy e está inteiro, mas nenhum quadro de verdade passou por ele. O primeiro teste de
> verdade é o item 1 da lista abaixo. Espere encontrar coisa: o `AcquireSync`/`ReleaseSync`
> do keyed mutex, o laço de eventos do MFT assíncrono e a criação das views do
> VideoProcessor são os três lugares onde erro de COM aparece só em execução.

## O que falta, em ordem

### 1. Rodar o encoder do Windows com captura real — **é o próximo passo**

Nada mais importa até isso acontecer. O caminho mais curto é o exemplo que já existe:

```powershell
cargo run -p capture --example spike -- 1080 10
```

Depois, o app inteiro: `npx tauri build` e clicar em Transmitir. Se falhar, o erro aparece
na tela (o caminho de erro já está certo). Os suspeitos, em ordem:

- `MFCreateDXGISurfaceBuffer` recusando a textura NV12 (device errado no gerente).
- O laço `bombear()` recebendo um evento que não é `NeedInput` nem `HaveOutput`.
- `CreateVideoProcessorInputView` reclamando do formato BGRA.

### 2. Áudio do sistema no Windows

`WindowsCapturer::audio_chunks_captured()` devolve `0` fixo. Áudio de sistema no Windows é
**WASAPI loopback**, não vem pelo Graphics Capture. O caminho depois disso já existe
inteiro: `AudioEncoder` (Opus em blocos de 20 ms) e o RTP puro. Falta só a fonte.

### 3. Menu hambúrguer com quem está na sala e quem está ao vivo

O dado já existe e chega: `sfu.peers` é um `Map` de `{name, sharing}` e o evento
`peersChanged` dispara a cada mudança. Hoje a interface só mostra a contagem — "N pessoas"
(`ui/app.js`, `refreshPeople`). É trabalho de interface, nada de protocolo.

### 4. Mutar o áudio de quem se assiste

Os elementos `<audio>` são criados em `App.consume` (`ui/app.js`). O jeito preguiçoso é
`audio.muted = true` num botão por quadro. O jeito certo é `pauseConsumer` — a ação já
existe no SFU e está testada —, que também para de gastar banda com um áudio que ninguém
ouve.

### 5. Miniatura do seletor no Windows

`WindowsCapturer::preview` devolve vazio. O seletor abre listando os nomes, sem imagem.
Cosmético.

### 6. Linux

`LinuxCapturer` roda o `gst-launch-1.0` como processo filho e lê H.264 Annex-B pelo pipe
(`crates/capture/src/linux.rs`); o `PlatformEncoder` do Linux só repassa. Falta Wayland
(`pipewiresrc` via portal), lista de janelas e keyframe sob demanda (hoje é um por segundo).

### 7. Pendências fora do código

- **nginx em produção ainda não foi trocado.** A config nova está em
  `/tmp/unkvoid-nginx.conf` na VPS e o backup em `/etc/nginx/sites-available/discord.bak`.
  **Sem isso o app novo não funciona**: ele pergunta `GET /health` antes de deixar entrar
  numa sala, e hoje esse caminho cai no Laravel e devolve 404 — a tela fica em
  "Reconectando…" para sempre.

  ```bash
  ssh vps 'sudo cp /tmp/unkvoid-nginx.conf /etc/nginx/sites-available/discord && sudo nginx -t'
  ssh vps 'sudo systemctl reload nginx'
  ```

- **`pm2 delete reverb`** — o processo continua rodando e serve o Laravel que já não existe.
- **Instalador do macOS.** Falta. `.dmg` só sai no macOS: lá, `cd native/apps/desktop &&
  npx tauri build --bundles app dmg`, e o resultado vai para `dist/`.
- **Assinatura.** Os instaladores em `dist/` saíram **sem assinatura**, porque a chave
  privada (`~/.tauri/unkvoid.key`) está no Mac. Eles instalam e rodam; o que não fazem é
  servir de alvo para a atualização automática. Veja [dist/README.md](dist/README.md).

## Como gerar os instaladores do Windows

Existem **duas cópias do repositório** nesta máquina, e a razão importa:

- `/var/www/projects/unkvoid` — no WSL. É o repositório de verdade, onde se edita e se
  commita.
- `C:\Users\edsu\unkvoid-build` — só o `native/`, no disco C:. É de onde o instalador sai.

A cópia existe porque o `node_modules` do WSL traz o `@tauri-apps/cli` **de Linux**: rodar
`npx tauri build` de lá pelo Windows não funciona. Na cópia, um `npm ci` do lado do Windows
traz o binário certo. Ela é descartável — para atualizar:

```bash
rsync -a --delete --exclude node_modules --exclude target --exclude dist \
    /var/www/projects/unkvoid/native/ /mnt/c/Users/edsu/unkvoid-build/native/
```

Depois, no PowerShell:

```powershell
cd C:\Users\edsu\unkvoid-build\native\apps\desktop
npm ci
npx tauri build --bundles nsis,msi
```

Sai em `native\target\release\bundle\` — o `-setup.exe` (NSIS) e o `.msi` —, e o app
solto, que roda sem instalar, em `native\target\release\unkvoid-desktop.exe`. Copie os
três para o `dist/` do repositório.

**Sem a chave de assinatura, acrescente `--config "{\"bundle\":{\"createUpdaterArtifacts\":false}}"`**
— o Tauri recusa gerar artefato de update sem a chave privada, e ela está no Mac.

Do WSL, o mesmo script chamado por fora:

```bash
cd /mnt/c && cmd.exe /c "C:\Users\edsu\build-msi.cmd"
```

## Esta máquina — o que já está montado

Isto poupa uma hora de quem chegar agora. O desenvolvimento é em **WSL (Ubuntu 26.04)**,
com o repositório em `/var/www/projects/unkvoid`, mas o Rust do Windows precisa rodar do
lado de lá.

**Instalado nesta sessão, do zero:**

| Onde | O quê |
|---|---|
| Windows | Visual Studio Build Tools 2022 (carga C++), MSVC 14.44, Windows SDK 10.0.26100 |
| Windows | Rust 1.98.1 (`C:\Users\edsu\.cargo\bin\cargo.exe`), Node 24.19 |
| WSL | Rust 1.98.1 + alvo `x86_64-pc-windows-msvc` |

**Como compilar o código do Windows.** O trabalho todo é `C:\Users\edsu\cargo-win.cmd`,
que aceita os mesmos argumentos do cargo. Direto no PowerShell ou no cmd do Windows:

```powershell
C:\Users\edsu\cargo-win.cmd check --workspace --all-targets
C:\Users\edsu\cargo-win.cmd clippy --workspace --all-targets -- -D warnings
C:\Users\edsu\cargo-win.cmd test --workspace
```

De dentro do WSL, onde o repositório mora, é o mesmo script chamado por fora:

```bash
cd /mnt/c && cmd.exe /c "C:\Users\edsu\cargo-win.cmd check --workspace --all-targets"
```

O `cd /mnt/c` é do **bash do WSL** e não existe no PowerShell — lá ele vira
`C:\mnt\c` e o comando falha antes de começar. No PowerShell, use a primeira forma.

O `cargo-win.cmd` faz três coisas que **não são opcionais**: mapeia o repositório da WSL
para `Y:` (o `cmd.exe` não aceita caminho UNC como diretório atual), põe o CMake que veio
dentro do Build Tools no PATH (o `opusic-sys` precisa dele e ele não está no PATH do
sistema), e aponta `CARGO_TARGET_DIR` para `C:\Users\edsu\unkvoid-target` — compilar dentro
da WSL pelo `Y:` falha no lock do compilador incremental.

Do lado do WSL dá para conferir só o `capture`, que é puro Rust:

```bash
cd native && cargo check --target x86_64-pc-windows-msvc -p capture
```

O `media` **não** dá: o `opusic-sys` compila C e precisa do MSVC, que não existe no Linux.

**Servidor:** a chave SSH está em `~/.ssh/vps` e o alias `vps` no `~/.ssh/config` do WSL.
Ela veio de `/mnt/c/Users/edsu/.ssh/nome_da_chave` — que é **diferente** da chave de mesmo
nome que já estava no WSL; só a do Windows autentica.

## Armadilhas já pagas

- **`npm run check` antes de qualquer commit no desktop.** Três vezes um script de
  substituição em bloco apagou um método inteiro do `app.js`. O sintoma é tela preta.
- **Teste no `harness.html`, não na janela do app.** A janela do Tauri não tem console: um
  erro de JS vira tela preta sem pista.
- **`use_sfu` só depois de declarar vídeo E áudio.** Ao contrário, o Rust manda RTP de um
  SSRC que o servidor ainda não conhece e ele descarta calado: a transmissão "funciona" e
  ninguém vê nada. O `check-broadcast.mjs` guarda essa ordem.
- **O crate `capture` tem um módulo chamado `windows`.** Dentro dele, `windows::Win32::…`
  acha o módulo local em vez da crate da Microsoft. Precisa de `::windows::`.
- **Ponteiro COM não é `Send`.** O encoder atravessa uma vez para a thread da captura, e há
  um `unsafe impl Send` com a justificativa escrita. Não é preguiça: os objetos do D3D11
  (com proteção multithread ligada) e o MFT assíncrono são livres de apartamento.
- **`hidden` do Tailwind é classe, não atributo.**
- **`build.rs` tem `cargo:rerun-if-changed=../dist`.** Sem isso o app sai com a interface
  da última vez que o Rust mudou.
- **Release não pode ser pré-lançamento.** O auto-update lê
  `/releases/latest/download/latest.json`, e o "latest" do GitHub ignora pré-lançamentos.
