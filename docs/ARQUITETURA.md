# Arquitetura

O mapa do Unkvoid: para que serve cada peça, como elas conversam e por onde passa cada
coisa. O detalhe de cada assunto mora em esta pasta, e cada seção aponta para o arquivo certo.
O contrato do que atravessa a rede é [docs/CONTRATO.md](CONTRATO.md).

## Para que o projeto existe

**Compartilhar a tela sem perder fps no jogo.** No navegador o encoder de vídeo roda na
CPU, o jogo e a compressão disputam o mesmo processador e a transmissão cai para 1 fps. O
app usa o chip de codificação da placa de vídeo:

```
captura → textura na GPU → encoder de hardware → 1 quadro → SFU → N espectadores
```

Três coisas sustentam o projeto, e mudança que quebre uma delas está desfazendo ele:

1. o quadro **não desce para a CPU** antes de ser comprimido;
2. é comprimido **uma vez**;
3. sobe **uma vez**: o servidor replica, então o upload de quem transmite não cresce com a
   plateia.

Em volta disso, para quem tem conta, um app no molde do Discord: servidores, cargos com
permissões, canais de texto e voz, câmera, chat, amigos e mensagens diretas.

### Os dois modos

| | Sala por código | Servidores |
|---|---|---|
| Conta | não precisa | precisa |
| Onde existe | só no SFU, enquanto tiver gente dentro | no banco do Laravel |
| Identidade | o código de 12 caracteres **é** a sala | conta, cargos e permissões |
| Laravel no caminho | não (só recebe o log de acesso) | decide tudo e assina o token |
| Compartilhar tela | qualquer um na sala | só de dentro da voz, com `STREAM` |

A sala por código é produto, não legado: mexer num modo não degrada o outro.

## Visão geral

```
                    Máquina de quem usa
 ┌──────────────────────────────────────────────────────┐
 │ App Unkvoid (native/)                                │
 │                                                      │
 │  Interface: React na webview                         │
 │    telas, chat, assistir por WebRTC                  │
 │            ▲  invoke (IPC do Tauri)  │               │
 │            │                         ▼               │
 │  Núcleo em Rust                                      │
 │    captura, encoder da GPU, RTP/SRTP, receptor Linux │
 └───┬──────────────────┬───────────────────────┬───────┘
     │ HTTPS            │ WSS                   │ UDP
     │ API, login,      │ chat e sinalização,   │ mídia: RTP/SRTP
     │ atualização      │ os dois pelo SFU      │ e WebRTC
 ┌───▼──────────────────▼───────────────────────▼───────┐
 │ VPS (unkvoid.com)                                    │
 │                                                      │
 │  nginx :443                                          │
 │   ├─ /  /api  /downloads          → Laravel (web/)   │
 │   ├─ /sfu  /health                → SFU :3000 (sfu/) │
 │   └─ /apt                         → APT, no MinIO    │
 │                                                      │
 │  SFU: Node (sinalização) + workers C++ (mídia)       │
 │       UDP 40000-40006 e 41000-42000, sem nginx       │
 │                                                      │
 │  Docker: MySQL · MinIO (s3.unkvoid.com) · e-mail     │
 └──────────────────────────────────────────────────────┘
        Laravel ⇄ SFU: HTTP assinado, nos dois sentidos
```

## As três peças

| Pasta | Para que serve | Tecnologia | Roda em | Dona de | Nunca faz |
|---|---|---|---|---|---|
| `native/` | tudo o que acontece na máquina de quem usa: capturar, comprimir, mandar, receber e mostrar | Rust; interface nativa por sistema (SwiftUI, Slint, GTK4) — e o app Tauri + React, até a paridade | Windows, macOS, Linux | interface, captura, encoder, mídia local | decidir permissão: só esconde botão |
| `sfu/` | relé de mídia: recebe cada transmissão uma vez e replica para quem assiste | Node 22 + mediasoup | VPS | salas, pessoas conectadas, producers, consumers, a mídia | decidir permissão: só confere a assinatura |
| `web/` | site, contas e tudo que precisa de banco | Laravel 13, Livewire 4, Flux, Sanctum | VPS | conta, servidor, cargo, canal, membro, mensagem, auditoria, versões do app | tocar em mídia |

Fora das três:

| Pasta | O que tem |
|---|---|
| `infra/` | nginx, `docker-compose.yml` (MySQL, MinIO, e-mail), sysctl de UDP, `deploy-web.sh`, instalação do runner |
| `.github/workflows/` | deploy do site e do SFU, build do Linux, release do Windows e do macOS |
| esta pasta | o detalhe de cada assunto (tabela no fim) |

## `native/`: o app

### Duas camadas

O **núcleo** é Rust e decide tudo. A **interface** desenha e nada mais. Entre os dois há uma
superfície pequena: comandos entram, eventos saem.

```
    Swift (macOS)  ──► ABI C ─┐
    Rust (Windows, Slint)  ───┼──► shared/core ──► capture · media · SFU · Laravel
    Rust (Linux, GTK)      ───┘
    React (Tauri)  ─┘   até a paridade
```

**A regra que sustenta o desenho:** regra de negócio não mora em pasta de sistema. Se uma
decisão aparecer em `apps/macos/`, ela terá de ser escrita de novo em `apps/windows/` e em
`apps/linux/` — e é assim que um app vira três apps com os mesmos defeitos em lugares
diferentes. O teste é direto: "o Windows vai precisar disto igual?" Se sim, sobe para o
`core`.

**Por que interface nativa, e não uma webview para os três** (decisão do dono, 20/09/2026):
o app roda ao lado de um jogo, e uma webview carrega um motor de browser inteiro para
desenhar botão. Some o custo de manter três motores diferentes (WebView2, WKWebView,
WebKitGTK) que se comportam de formas distintas — o do Linux nem traz WebRTC, e é daí que
nasceu o receptor nativo. O preço é escrever a tela três vezes; o que **não** se escreve três
vezes é a lógica, que é o grosso.

**Por que isto custou menos do que parecia:** a parte cara de um app de voz e tela é a
mídia, e ela já era nativa antes da decisão — captura na GPU, encoder por hardware, envio
RTP+SRTP (`plain.rs`) e recepção (`receiver.rs`). O caminho sem WebRTC já existia, escrito
para o Linux.

### A ponte para o Swift

Uma ABI C pequena, em `shared/core/src/ffi.rs` — o que ela tem, ação por ação, está em
`docs/CONTRATO.md`. **Só o macOS passa por ela**: o Linux (GTK) e o Windows (Slint) são Rust
e usam o `core` como crate, sem JSON no meio e sem liberar ponteiro à mão.

| Função | O quê |
|---|---|
| `unkvoid_core_new` / `unkvoid_core_free` | cria e libera o núcleo |
| `unkvoid_connect` | abre o socket do SFU |
| `unkvoid_call` | uma ação do SFU. **Bloqueia** até a resposta — nunca na thread que desenha |
| `unkvoid_app` | as decisões do app: tela, sala, voz, compartilhar, conta, servidores, mensagens. Também bloqueia no que fala com o servidor |
| `unkvoid_next_event` | o próximo aviso da fila (sala, chat, presença), sem bloquear |
| `unkvoid_next_media` | o próximo quadro H.264 ou bloco de som do que se assiste, para uma thread só da interface |
| `unkvoid_speak` / `unkvoid_show` | o microfone e a câmera que a interface captura, entrando no núcleo (PCM; `IOSurface`, sem cópia) |
| `unkvoid_string_free` / `unkvoid_bytes_free` | devolvem o que o núcleo alocou — uma vez só |

O `Handle` é seguro para uso concorrente: tudo atrás de `Mutex`, e as funções tomam `&`. A
interface consulta eventos num timer **enquanto** uma ação está em voo, e com `&mut` isso
seria corrida de dados.

**Erro que chega à tela nunca carrega caminho, endereço nem código de status.** O núcleo
devolve um motivo (`unreachable`, `signedOut`, `notAllowed`, `gone`, `invalid`,
`serverBroke`, `tooFast`) e cada interface escreve a frase; o detalhe vai para o log. A
única exceção é erro de validação, que o Laravel já devolve em português e falando do campo
que a pessoa digitou. Ver `shared/core/src/failure.rs`.

### Pastas

| Caminho | O que faz |
|---|---|
| `shared/capture/` | captura de tela e do som do sistema, um arquivo por sistema (`macos.rs`, `windows.rs`, `windows_audio.rs`, `linux.rs`, `linux_audio.rs`) |
| `shared/media/` | encoder por hardware (`windows.rs`, `macos.rs`), Opus (`audio.rs`), envio RTP/SRTP para o SFU (`plain.rs`), recepção (`receiver.rs`) e o caminho de volta do RTP: quadro H.264 inteiro e Opus → PCM (`unpack.rs`) |
| `shared/core/` | **a lógica, compartilhada pelas quatro interfaces**: protocolo e cliente do SFU, sessão e lista de quem está na sala, a sala viva de quem vem pela ABI (`room.rs`: publicar, assistir, microfone, câmera), assistir sem GStreamer (`watching.rs`), cliente da API do Laravel, código de sala, estado do app, motivos de erro, mapa de teclas e a ABI C |
| `shared/storage/` | o estado em disco na pasta do sistema, com o token cifrado em AES-256-GCM |
| `apps/macos/` | Swift + SwiftUI |
| `apps/windows/` | Rust + Slint, sem ponte |
| `apps/linux/` | Rust + GTK4, sem ponte |
| `apps/desktop/src-tauri/src/lib.rs` | os comandos do Tauri, janela, bandeja, atualização: a ponte entre interface e crates |
| `…/broadcast.rs` | liga captura → encoder → transporte; microfone e câmera pelo Rust no Linux |
| `…/watch.rs` | assistir sem WebRTC (Linux): RTP puro → GStreamer → MJPEG em `127.0.0.1` → `<img>` |
| `…/login.rs` | login com Google pelo navegador do sistema, de volta por `unkvoid://` |
| `…/shortcuts.rs` | atalhos globais (falar apertando, mutar) |
| `…/logbook.rs` | log em arquivo: a janela não tem console |
| `apps/desktop/ui/core/` | a lógica da interface, sem React: uma classe por assunto (abaixo) |
| `apps/desktop/ui/components/` | as telas em React, lendo o estado das classes de `ui/core` |
| `apps/desktop/ui/dev/DevTauriBridge.ts` | finge o Rust, para rodar a interface num navegador comum |
| `apps/desktop/tests/` | `static` (checks de idioma e de comentário), `unit` (um arquivo por área) e `integration` (os clientes do app contra a pilha local), em Vitest |
| `tests/linux/` | contêiner Ubuntu com os cenários do Linux |

As classes de `ui/core`:

| Classe | Papel |
|---|---|
| `App` | raiz: escolhe a tela (`update`, `offline`, `entry`, `room`, `hub`), avisos, log, atualização e a sala por código |
| `Hub` | o modo servidor: `ApiClient`, a conexão de tempo real com o SFU (`Realtime`) e os filhos `Chat` (um para o canal de texto aberto, outro para o chat da voz), `Voice`, `ServerSettings`, `Friends`, `Direct`; ao reconectar, busca de novo o que perdeu |
| `SfuClient` | o WebSocket do SFU e o `Device` do mediasoup-client; reconexão com espera sorteada, e retomada que compara a lista de pessoas e reproduz o que se perdeu |
| `Media` | os cartões de quem transmite: consome por WebRTC ou pelo receptor nativo |
| `Sharing`, `Broadcast` | o seletor de tela e a publicação da tela nativa no SFU |
| `Voice`, `Mic` | entrar na voz com token, microfone, câmera, detecção de fala (pelo `AnalyserNode`, ou pelo `voice:level` do Rust no Linux) |
| `Chat`, `ImageShrinker` | mensagens do canal, e a redução da imagem para caber em 2 MB antes de enviar |
| `ApiClient` | HTTP para o Laravel com o token do Sanctum |
| `Permissions` | os bits de permissão, só para esconder botão |
| `Store` | estado observável que os componentes assinam |

### Por sistema

| | macOS | Windows | Linux |
|---|---|---|---|
| Captura de tela | ScreenCaptureKit | Windows Graphics Capture (textura Direct3D 11) | `gst-launch-1.0` com `ximagesrc` (X11) ou `pipewiresrc` pelo portal ScreenCast (Wayland) |
| Som do sistema | ScreenCaptureKit, sem o som do próprio app | WASAPI por processo: só o jogo, sem o Discord e sem o app | monitor do PulseAudio/PipeWire |
| Encoder | VideoToolbox | Media Foundation: NVENC, QuickSync, VCE; sem nenhum, software em 720p30 | `nvh264enc`, `vah264enc` ou `vaapih264enc`; sem nenhum, `x264enc` |
| Microfone e câmera | `getUserMedia` da webview (WebRTC) | `getUserMedia` da webview (WebRTC) | Rust: `pulsesrc` e `v4l2src` → RTP puro |
| Assistir | WebRTC da webview | WebRTC da webview | receptor nativo: RTP puro → GStreamer → MJPEG |
| Atualização | automática | automática (`.exe` e `.msi`) | pelo APT |

## `sfu/`: o relé de mídia

### Duas camadas no mesmo serviço

| Camada | O que faz | Onde roda |
|---|---|---|
| Sinalização | entrar, publicar, consumir, sair: só troca mensagens | JavaScript, no processo Node |
| Mídia | mover pacote de vídeo e áudio | workers do mediasoup: processos C++, um por núcleo |

O Node não vê um único pacote de vídeo. Por que o SFU é Node, e não PHP, Java ou Rust:
[docs/DECISOES.md](DECISOES.md).

### Conceitos

| Nome | O que é |
|---|---|
| `Room` | uma sala. O id é o código da sala por código (3 a 32 caracteres) ou o ULID de um canal de voz (26). A sala inteira mora num worker só, escolhido pelo que tem menos salas |
| `Peer` | uma pessoa conectada: um WebSocket, com uma `resumeKey` para voltar depois de cair |
| producer | uma origem que alguém manda: `screen`, `screenAudio`, `mic` ou `camera` |
| consumer | a cópia de um producer indo para uma pessoa; nasce pausado |
| transporte WebRTC | quem assiste pela webview, e mic/câmera no Windows e macOS. Uma porta por worker (40000-40006), multiplexada, UDP com TCP de reserva |
| transporte plain | RTP puro: o app transmitindo a tela, e o Linux recebendo. Uma porta por sentido, a partir de 41000. O `comedia` aprende o endereço de quem manda pelo primeiro pacote, então ninguém precisa abrir porta em casa |

### Pastas

| Caminho | O que faz |
|---|---|
| `src/app.ts` | monta o Express e o WebSocketServer no mesmo servidor: WebSocket em `/sfu`, heartbeat de 15 s, teto de conexões novas por IP |
| `src/server.ts` | sobe os workers e faz o `listen` |
| `src/Routers/HttpRouter.ts`, `Controller/HealthController.ts`, `RoomController.ts`, `Middleware/VerifySignature.ts` | o HTTP em Express: `/health`, e `/presence` e `/rooms/:room/{kick,mute}` assinados |
| `src/Routers/WebSocketRouter.ts`, `Request/`, `Controller/` | cada ação do WebSocket no molde do Laravel: rota → Request (valida) → Controller (age e devolve o objeto da resposta) |
| `src/Services/RoomRegistry.ts` | os workers e em qual deles cada sala mora |
| `src/Services/Room.ts`, `Peer.ts` | a sala, as pessoas, a carência de 30 s ao cair (a retomada devolve a lista com quem está na carência), uma sessão por conta |
| `src/Services/Signature.ts` | confere o token HMAC e a assinatura do HTTP do Laravel |
| `src/Services/Webhook.ts` | avisa o Laravel (`joined`, `left`) sem nunca segurar o `join` |
| `src/Config/index.ts` | tudo que vem do ambiente, os codecs e as portas |
| `check.mjs` | o contrato inteiro contra um servidor no ar, inclusive webhook e heartbeat |
| `ecosystem.config.cjs` | o pm2 da VPS: o SFU |

O WebSocket fala num envelope `{ id, action, data }`. As ações são `join`, `leave`,
`removePeer`, `createTransport`, `connectTransport`, `produce`, `producePlain`,
`pauseProducer`, `resumeProducer`, `closeProducer`, `consume`, `consumePlain`,
`resumeConsumer`, `pauseConsumer` e `closeConsumer`. Os eventos e os formatos estão no
contrato.

## `web/`: o Laravel

### Para que serve

| Frente | O que entrega |
|---|---|
| Site | página inicial com os downloads, cadastro, login (e-mail ou Google), recuperar senha, privacidade e termos |
| API do app (`/api`, Sanctum) | conta, servidores, cargos, canais, sobrescritas, membros, banimentos, mensagens, amigos, mensagens diretas, token de voz, token da sala por código, auditoria, `GET /api/config` |
| Tempo real (SFU) | `channel.{ulid}` (mensagens e voz de um canal), `server.{id}` (quem está online e mudança de estrutura), `user.{id}` (amigos, DMs, expulsão). O socket não reentrega o que se perdeu numa queda: o app busca de novo pela API ao reconectar |
| Distribuição | `/downloads/latest.json` (manifesto da atualização automática), `/downloads/{sistema}`, `POST /api/releases` assinado |
| Registro sem tela | `POST /api/errors` (erros enviados pelos apps) e `guest_accesses` (visitantes da sala por código) gravam no banco; o painel `/admin` que os mostrava saiu, e por enquanto não há tela nenhuma de administração |
| Webhook do SFU | `POST /api/sfu/events`, assinado |

### Pastas

| Caminho | O que faz |
|---|---|
| `app/Http/Controllers/Api/` | um controller por recurso |
| `app/Http/Requests/`, `app/Http/Resources/` | validação da entrada e formato da resposta |
| `app/Http/Middleware/` | `VerifySfuSignature` e `VerifyReleaseSignature` |
| `app/Models/` | o modelo de dados (abaixo) |
| `app/Events/` | o que o Laravel publica no SFU |
| `app/Services/Sfu/SfuClient.php` | assina o token de voz e chama o SFU (`kick`, `mute`, `presence`) |
| `app/Services/Storage/BucketService.php` | o bucket do MinIO |
| `app/Livewire/` | as páginas do site (entrar, cadastro, senha, início) |
| `routes/api.php`, `web.php` | a API e as páginas; a autorização dos canais é o `POST /api/sfu/authorize` |

### Modelo de dados

| Assunto | Tabelas |
|---|---|
| Conta | `users`, `personal_access_tokens`, `sessions`, `password_reset_tokens`, `files` (toda imagem enviada) |
| Social | `friendships`, `direct_messages` |
| Servidor | `servers`, `server_roles`, `server_members`, `member_roles`, `channels`, `channel_overwrites`, `messages`, `message_files`, `server_bans` |
| Voz | `channel_accesses` (entrada e saída de cada voz), `guest_accesses` (visitantes da sala por código) |
| Auditoria | `audits` (pacote `owen-it/laravel-auditing`) e `channel_audits` (o id do canal é ULID, e a coluna da `audits` é numérica) |
| Operação | `releases`, `error_reports`, as tabelas do `spatie/laravel-permission`, `jobs`, `cache` |
| Órfã | `clips`: os clipes saíram do código, e apagar a tabela é decisão do dono (migration) |

**Permissão** é um conjunto de bits calculado na ordem do Discord: dono ou
`ADMINISTRATOR` pode tudo; senão `@everyone` mais os cargos, depois as sobrescritas do
canal (`@everyone`, cargos, membro), e a hierarquia de cargos decide em quem se pode mexer.
Canal sem `VIEW_CHANNEL` nem aparece. A regra completa está em
[docs/CONTRATO.md](CONTRATO.md#permissões-bits-ubigint).

## Como as peças conversam

| De → para | Por onde | Quem prova quem é | Para quê |
|---|---|---|---|
| interface → Rust | `invoke` do Tauri | mesmo processo | captura, encoder, envio, recepção nativa, login |
| app → Laravel | HTTPS, JSON | `Authorization: Bearer` do Sanctum | tudo do modo servidor |
| app → SFU | WSS, o mesmo socket da sinalização | `identify` com o token de `POST /api/sfu/session`, e `subscribe` por canal | chat e presença em tempo real |
| app → SFU | WSS em `/sfu` | token HMAC de 60 s assinado pelo Laravel, ou nenhum (sala por código) | sinalização |
| app ⇄ SFU | UDP | chave SRTP (RTP puro) ou DTLS (WebRTC) | a mídia |
| Laravel → SFU | HTTP assinado (`x-unkvoid-timestamp`, `x-unkvoid-signature`) | `SFU_SECRET` | expulsar, mutar, presença |
| SFU → Laravel | webhook `POST /api/sfu/events` assinado | `SFU_SECRET` | quem entrou e saiu |
| navegador → app | `unkvoid://login?token=&state=` | `state` sorteado pelo app | voltar do login com Google |
| app → Laravel | `GET /downloads/latest.json`, `POST /api/errors` | nenhum (teto por IP) | atualização e relatório de erro |
| build → Laravel | `POST /api/releases` assinado | `RELEASE_SECRET` | publicar uma versão |

O `SFU_SECRET` é o mesmo nos dois lados. Sem ele (ou com menos de 32 caracteres) o SFU não
sobe: é melhor fora do ar do que aberto.

## Os fluxos

### 1. Sala por código

1. A pessoa escreve o nome e clica em **Criar uma sala**. O app sorteia 12 caracteres de
   `a-z0-9` (`RoomCode`).
2. O app confere `GET /health` antes de deixar entrar.
3. Abre o WebSocket e manda `join` **sem token**. O SFU cria a sala se ela não existir e
   dá à pessoa `guest:<installId>`, com tudo liberado. Quem está logado pede antes um
   token em `POST /api/rooms/{code}/token`, para valer a regra de uma sessão por conta.
4. O webhook `joined` vira uma linha em `guest_accesses` (aba "Visitantes" da auditoria).
5. A sala some quando o último sai.

O que protege: o teto de conexões novas por IP, e o `join` sem token recusar código de
26 caracteres, que é o formato do id de canal de voz.

### 2. Entrar numa voz de servidor

1. O app pede `POST /api/channels/{id}/voice/token`. O Laravel confere `CONNECT` e o
   limite de pessoas e assina um token de 60 s com o que a pessoa pode produzir (`can`:
   `speak`, `stream`, `video`).
2. O app manda `join` com o token. O SFU confere a assinatura e o vencimento, e derruba
   qualquer outra sessão da mesma conta (`replaced`).
3. O SFU avisa o Laravel (`joined`). O Laravel registra em `channel_accesses` e emite
   `VoiceStateUpdated`, e os outros veem a pessoa na voz.
4. O microfone entra **mutado**. No Windows e no macOS vai por WebRTC (`produce`); no
   Linux pelo Rust (`producePlain`).
5. Se a sinalização cair, o app espera um tempo sorteado (para a sala não voltar toda no
   mesmo instante), pede token novo e volta. Dentro de 30 s a sessão é retomada pela
   `resumeKey`, sem derrubar a mídia, e a resposta traz a lista de pessoas (inclusive quem
   está na carência): o app compara com a que tinha e reproduz o que perdeu na queda.

### 3. Compartilhar a tela: o caminho do quadro

```
start_broadcast   captura → textura na GPU → encoder de hardware
                  (H.264, sem B-frames, quadro-chave a cada 1 ou 2 s conforme o sistema)
sfu_offer         o Rust escolhe SSRC, tipo de payload e chave SRTP
producePlain      o SFU cria o transporte plain e devolve a porta e a chave dele
use_sfu           o Rust empacota RTP (MTU 1200), cifra SRTP e manda por UDP
SFU               aprende o endereço no primeiro pacote e replica para cada consumer
```

- O som da tela vai junto: Opus 48 kHz estéreo feito no Rust, no mesmo transporte, com
  SSRC próprio (um SSRC por origem).
- Perda entre o app e o SFU: o SFU pede o pacote de volta (NACK por SRTCP) e o app reenvia do
  histórico; se não der, pede quadro-chave (PLI) e o app atende. Entre o SFU e quem assiste, o
  mediasoup retransmite.
- A taxa acompanha a perda: muito NACK numa janela e o app baixa o alvo do encoder, até 35% da
  taxa da qualidade; perda sumindo, sobe de novo. Por que não REMB:
  [DECISOES.md](DECISOES.md#a-taxa-do-vídeo-acompanha-a-perda-e-não-o-remb).
- Transmissão que passa 30 s sem pacote é derrubada, e o SFU avisa quem transmite
  (`producerDead`). O mesmo aviso, com `reason: 'revoked'`, sai quando a retomada de uma sessão
  chega com um token que já não deixa transmitir.
- Taxa por qualidade: 720p60 a 5 Mb/s, 1080p60 a 10, 1440p60 a 20. Os ajustes de buffer e
  de encoder medidos estão em [docs/REDE.md](REDE.md).

### 4. Assistir

- **Windows e macOS:** o mediasoup-client na webview cria um transporte WebRTC, `consume`
  e `resumeConsumer`. Entra pelas portas 40000-40006.
- **Linux:** `watch_key` + `consumePlain`. Um `PlainReceiver` por sessão recebe tudo num
  socket só e separa por SSRC; o GStreamer decodifica; o quadro vira MJPEG num servidor
  local em `127.0.0.1`, lido por uma `<img>`; o som vai direto para a saída.
- O evento `watchers` diz quem está olhando cada tela. Conta só `screen`, e só consumer
  ativo.

### 5. Chat e tempo real

1. `POST /api/channels/{id}/messages`: o Laravel confere `SEND_MESSAGES` e grava. Até 3 imagens
   por mensagem, guardadas no bucket privado. Canal de voz também tem chat, pelas mesmas rotas.
2. `MessageSent` vai para `channel.{ulid}`, e o SFU só deixa assinar quem tem
   `VIEW_CHANNEL`.
3. Mudou a estrutura do servidor (canal, cargo, membro): `ServerUpdated` no
   `server.{id}`, e o app refaz o `GET`.
4. Amigos, mensagens diretas e expulsão chegam no `user.{id}`.
5. O socket caiu e voltou (rede, ou o deploy, que reinicia o SFU): nada do que passou é
   reentregue, então o app se identifica de novo, reinscreve os canais que ouvia e busca os
   servidores, a árvore aberta, amigos, conversas e as 50 mensagens mais recentes do canal e
   da conversa abertos, emendando com o que já estava na tela.

### 6. Moderação: mutar, expulsar, banir

O Laravel decide, confere a hierarquia e grava. Depois chama o SFU pelo HTTP assinado:
`mute` pausa o producer do microfone, `kick` fecha o socket. `MemberRemoved` no
`user.{id}` faz o app sair do servidor. Um token de voz emitido antes da expulsão
ainda vale 60 s, então o `joined` de quem já não é membro dispara um `kick` na hora.

### 7. Login com Google no app

1. `google_login` abre o navegador do sistema em `/oauth2/app?state=…`.
2. Google → Laravel → redireciona para `unkvoid://login?token=&state=`.
3. O sistema abre uma segunda cópia do app; pelo `single-instance`, ela repassa o endereço
   para a cópia que já estava aberta e sai.
4. O Rust confere o `state` e entrega o token do Sanctum à interface.

### 8. Atualização e publicação

- O app procura versão nova ao abrir e a cada 6 h, em `/downloads/latest.json`. O Laravel
  monta o manifesto a partir da tabela `releases`, com URLs do MinIO assinadas por 1 h. O
  app confere a assinatura minisign (a chave pública vai dentro do `tauri.conf.json`),
  instala e reinicia, **nunca dentro de uma sala**.
- Publicar Windows e macOS: o `release.yml` compila num runner do GitHub a partir do repositório
  (tag `v*` para o Windows, disparo à mão para o macOS; o build local é só para testar) →
  `publish-release.sh` → `POST /api/releases` assinado → instalador no MinIO e linha em
  `releases`. Esse é o único caminho: tirar uma versão ruim do ar é direto no banco.
- Linux: `build-linux.yml` no runner da VPS gera o `.deb` e publica no repositório APT em
  `/apt`, assinado com GPG. Quem atualiza é o `apt`.

As duas chaves e o que acontece se trocar uma: [docs/AUTO-UPDATE.md](AUTO-UPDATE.md).

## Onde roda

### Produção: uma VPS

| Processo | Porta | Observação |
|---|---|---|
| nginx | 80, 443 | TLS; distribui por caminho (visão geral) e serve `s3.unkvoid.com` para o MinIO |
| Laravel (php-fpm 8.4) | atrás do nginx | cada deploy numa pasta nova; o `current` troca quando tudo está pronto |
| SFU | `127.0.0.1:3000` + UDP | pm2, 7 workers (um por núcleo menos um) |
| MySQL 8.4 | `127.0.0.1:3307` | Docker |
| MinIO | `127.0.0.1:9000` | Docker; instaladores, fotos, ícones e o repositório APT |
| e-mail | as do e-mail | Docker (`docker-mailserver`) |
| runner do GitHub Actions | — | faz os deploys e o build do Linux |

Portas que o firewall do painel precisa abrir:

| Porta | Protocolo | Para quê |
|---|---|---|
| 80, 443 | TCP | site, API, WebSockets |
| 40000-40006 | UDP e TCP | quem assiste por WebRTC (uma por worker; TCP é a reserva de rede que bloqueia UDP) |
| 41000-42000 | UDP | RTP puro: quem transmite, e quem assiste no Linux |

Porta fechada não dá erro: a transmissão "funciona" e ninguém vê nada. Por que a faixa é
larga: [docs/UDP.md](UDP.md). A máquina, as medições e o que desligar:
[docs/SERVIDOR.md](SERVIDOR.md). Levantar do zero: [docs/INSTALAR-VPS.md](INSTALAR-VPS.md).

### Deploy

| Workflow | Quando | O que faz |
|---|---|---|
| `deploy-web.yml` | push na `main` que mexe em `web/` | `infra/deploy-web.sh` no runner: release nova, migrations, recarrega o php-fpm |
| `deploy-sfu.yml` | push na `main` que mexe em `sfu/` | `git reset --hard` no clone da VPS, build, lint e `install.sh`, que **reinicia na hora** — quem está em chamada cai por alguns segundos |
| `build-linux.yml` | push na `main` que mexe em `native/` | o `.deb`, publicado no APT |
| `release.yml` | tag `v*` ou disparo manual | Windows e macOS nos runners do GitHub, publicados pela API assinada |

### Local

Os comandos para subir as três peças e as verificações antes de entregar estão no
[CLAUDE.md](../CLAUDE.md) e no fim de [docs/CONTRATO.md](CONTRATO.md).

## Segurança, em uma linha por camada

| Camada | O que protege |
|---|---|
| Mídia | SRTP (AES-128 + HMAC-SHA1) com chave sorteada por transmissão; WebRTC com DTLS |
| Entrada no SFU | token HMAC de 60 s com o que a pessoa pode produzir; o SFU recusa o resto |
| Chamadas entre Laravel e SFU | assinatura com o `SFU_SECRET` e janela de 300 s |
| Sala por código | o código é a única credencial; teto de conexões novas por IP |
| Atualização | assinatura minisign conferida pelo próprio app; APT assinado com GPG |
| Imagens e instaladores | bucket privado, toda URL assinada e com prazo |

O que não está protegido, e por quê: [docs/SEGURANCA.md](SEGURANCA.md).

## Glossário

| Termo | O que é |
|---|---|
| SFU | *Selective Forwarding Unit*: servidor que recebe a mídia de cada um e repassa para os outros sem decodificar |
| producer / consumer | no mediasoup, a mídia que entra de alguém / a cópia que sai para alguém |
| RTP puro (*plain*) | mídia em RTP direto sobre UDP, sem ICE nem DTLS: o app já combinou SSRC e chave com o servidor |
| SRTP | RTP cifrado e autenticado |
| SSRC | o número que identifica um fluxo dentro do RTP; um por origem |
| `comedia` | o servidor aprende o endereço de quem manda pelo primeiro pacote que chega |
| PLI | pedido de quadro-chave, mandado quando falta pacote |
| ULID | o id dos canais: 26 caracteres, ordenável por tempo |
| Sanctum | o token de API do Laravel que o app guarda |

## Onde está o resto

| Arquivo | Para quê |
|---|---|
| [docs/CONTRATO.md](CONTRATO.md) | o contrato: rotas, token, ações e eventos do SFU, webhook, comandos do Tauri |
| [docs/ESTADO.md](ESTADO.md) | o que só foi escrito sem rodar em hardware, o que falta, as perguntas abertas |
| [docs/DECISOES.md](DECISOES.md) | o que foi decidido e por quê |
| [docs/REDE.md](REDE.md) | o caminho da imagem e cada ajuste medido |
| [docs/UDP.md](UDP.md) | as faixas de porta e o que quebra calado |
| [docs/SEGURANCA.md](SEGURANCA.md) | modelo de ameaça |
| [docs/SERVIDOR.md](SERVIDOR.md), [docs/INSTALAR-VPS.md](INSTALAR-VPS.md) | a VPS que existe e como levantar outra |
| [docs/AUTO-UPDATE.md](AUTO-UPDATE.md) e `docs/BUILD-*.md` | publicar e buildar por sistema |
