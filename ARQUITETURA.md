# Unkvoid — arquitetura

O que cada peça faz e **por que ela existe**. Para o estado do trabalho (o que falta, as
armadilhas já pagas, o deploy), veja [CONTINUIDADE.md](CONTINUIDADE.md).

---

## O desenho em uma frase

Três programas, com uma divisão simples: **o Laravel decide quem pode o quê**, **o SFU
move mídia** e **o app nativo captura tela sem passar por navegador**. Vídeo nunca toca
o PHP.

```
                    ┌───────────────────────────────┐
   navegador ──────►│  web/    Laravel 13 + Livewire│  identidade, permissão, chat
                    │          discord.unkvoid.com  │  emite os tokens
                    └───────────────┬───────────────┘
                          token JWT │ (HS256, curto)
                    ┌───────────────▼───────────────┐
   navegador ◄─────►│  sfu/    Node + mediasoup     │  áudio e vídeo de verdade
   app       ──────►│          4 workers            │  presença e sinalização
                    └───────────────────────────────┘
                                    ▲
                    ┌───────────────┴───────────────┐
                    │  native/ Rust + Tauri         │  captura de tela nativa
                    │          app de desktop       │  encoder por hardware
                    └───────────────────────────────┘
```

O token é a única coisa que atravessa a fronteira: o SFU **não** consulta o banco e não
sabe o que é um servidor ou um convite. Ele confere a assinatura, lê `room`, `sub` e
`role`, e trata todo o resto como consequência. Isso é o que permite reiniciar o SFU sem
tocar no Laravel, e vice-versa.

---

## `web/` — Laravel 13 + Livewire 4

A fonte da verdade sobre pessoas, servidores, canais e mensagens. Também é quem emite os
tokens do SFU: **nenhuma outra peça decide permissão**.

### Domínio

| Arquivo | Papel |
|---|---|
| `app/Models/{User,Server,ServerMember,Channel,Message}.php` | as cinco tabelas. Chave primária é **UUID** em todas |
| `app/Actions/Servers/CreateServer.php` | cria servidor + dono + `#geral` (texto) + `Geral` (voz) numa transação |
| `app/Actions/Servers/JoinServerByInvite.php` | entrada por `invite_code` |
| `app/Actions/Auth/ResolveGoogleUser.php` | casa a conta do Google por `google_id`, senão por e-mail |
| `app/Http/Middleware/RequireNickname.php` | primeiro acesso sem apelido cai no onboarding |

### Interface

`app/Livewire/Workspace/Shell.php` é **o workspace inteiro** — trilha de servidores,
lista de canais, chat, membros, modais e moderação. Um componente só, de propósito:
trocar de canal é estado do Livewire e não navegação, porque **navegar derrubaria a
chamada de voz**.

O `resources/views/livewire/workspace/shell.blade.php` carrega `wire:ignore` em tudo que
o JavaScript é dono: o palco de voz, os `<video>`, a lista de participantes. Sem isso o
Livewire recria o trecho a cada render e leva a chamada junto.

### Emissão de tokens

| Arquivo | Emite |
|---|---|
| `app/Support/SfuToken.php` | o JWT: `sub` (usuário), `room` (servidor+canal), `role` |
| `app/Http/Controllers/VoiceTokenController.php` | token de sala — confere associação, tipo do canal e dono |
| `app/Http/Controllers/PresenceTokenController.php` | token só de leitura, para ver quem está nos canais sem entrar |

### API do desktop

`routes/api.php`, autenticada por Sanctum. Login por e-mail/senha, login pelo Google via
deep link, servidores, canais, mensagens e os **mesmos** emissores de token que o web usa.

`app/Http/Controllers/Api/DesktopAuthController.php` faz o Google fora do app: abre no
navegador do sistema e volta por `discord2://auth?token=…`. A senha nunca passa pela
janela do app.

### Tempo real do chat

| Arquivo | Papel |
|---|---|
| `app/Events/MessageSent.php` | anuncia a mensagem. Carrega **só o id** — quem recebe lê do banco com as permissões de sempre |
| `routes/channels.php` | quem pode escutar `channel.{id}`: só membro do servidor dono |
| `resources/js/voice/ChatSocket.js` | assina o canal aberto e pede refresh ao componente |
| `resources/js/echo.js` | monta o Echo lendo a configuração **do HTML**, não do bundle |
| `reverb.config.cjs` | o daemon sob pm2. A porta sai do `.env` |

### Cliente de mídia (`resources/js/voice/`)

| Arquivo | Papel |
|---|---|
| `SfuClient.js` | protocolo do SFU: entrar, transportes, publicar, consumir, reconectar com backoff, trocar qualidade sem republicar |
| `VoiceStage.js` | a interface da chamada: grade, tela cheia, controles por transmissão, presença, cronômetro |
| `MicrophoneGate.js` | decide **quadro a quadro** se o áudio sai: detecção de voz ou apertar-para-falar |
| `PresenceClient.js` | quem está em cada canal de voz, por WebSocket, mesmo para quem não entrou |
| `ChatSocket.js` | o chat em tempo real |

Os três primeiros são compartilhados com o app desktop, por alias do Vite — não copiados.

---

## `sfu/` — Node + TypeScript + mediasoup

A API de mídia. Estrutura igual à de um Laravel, de propósito: **rota → Request →
Controller → Service → Resource**, e toda resposta sai por um Resource.

```
src/
  Enums/       Action, Role, Source          o vocabulário do protocolo
  Exceptions/  ApiException                  422 / 401 / 403 / 404 com significado
  Http/
    routes.ts        cada ação → Request + Controller (guest: true é a única aberta)
    Kernel.ts        despacha, captura exceção e devolve status
    Server.ts        WebSocket + /health
    Requests/        validação e acessores — o controller não lê `data` cru
    Controllers/     fino, sem lógica
    Resources/       o formato da resposta, em um lugar só
  Services/
    RoomRegistry.ts  os workers e a distribuição de salas
    Room.ts          uma sala: transportes, produtores, retomada de sessão
    Peer.ts          um participante e a mídia dele
    PresenceRegistry.ts  quem está onde, para quem está de fora
    TokenVerifier.ts     confere a assinatura do Laravel
```

### O que cada Service resolve

**`RoomRegistry`** — um worker do mediasoup é um **processo C++ de uma thread só**: satura
um core e para. Threads de Node não ajudariam, porque mídia nunca passa por JavaScript.
Escalar aqui é um worker por core e distribuir as salas. Cada worker tem sua própria porta
de mídia porque o `WebRtcServer` não é compartilhável entre processos.

**`Room`** — guarda o essencial: **a queda do WebSocket não derruba a mídia**. O socket é
só sinalização; os transportes WebRTC continuam vivos. Quem cai tem 45 segundos para
voltar e retomar a sessão. É isso que faz um deploy do SFU não cortar as chamadas.

**`Peer.removePeer` recebe o objeto, não o id** — fechar o socket de uma sessão
substituída não pode derrubar a sessão nova, que carrega o mesmo id de participante.

### As ações

| Ação | Para quê |
|---|---|
| `join` / `leave` | entrar e sair da sala (a única aberta é `join`, que traz o token) |
| `createTransport` / `connectTransport` | os canos WebRTC de subida e descida |
| `produce` / `closeProducer` | publicar microfone, tela ou áudio da tela |
| `producePlain` | **publicar por RTP puro** — é como o app nativo transmite sem WebRTC |
| `consume` / `resumeConsumer` / `pauseConsumer` | receber, pausar sem desconectar |
| `signal` | relé entre dois participantes: é por aqui que o P2P do app se acerta |
| `watchServer` | presença sem entrar em canal nenhum |
| `stopBroadcast` / `disconnectPeer` | moderação em níveis: encerrar transmissão ≠ tirar da chamada ≠ tirar do servidor |

```bash
cd sfu && pnpm run check   # asserções sobre o contrato inteiro, incluindo o RTP puro
```

---

## `native/` — Rust + Tauri

O app de desktop. Existe por um motivo só: **capturar a tela sem passar pelo navegador**,
com o encoder da placa de vídeo, para não pesar enquanto se joga.

```
native/
  crates/
    capture/   captura de tela e áudio do sistema, por plataforma
    media/     encoder por hardware, Opus, WebRTC e RTP puro
  apps/desktop/
    src-tauri/ as pontes entre a interface e o Rust
    ui/        a interface (HTML/JS), pelo Vite
```

### `crates/capture`

| Arquivo | Papel |
|---|---|
| `lib.rs` | `Quality`, `CaptureConfig`, `VideoFrame` e o tipo `GpuSurface` por plataforma |
| `macos.rs` | ScreenCaptureKit — funciona |
| `windows.rs` | Windows Graphics Capture — compila, nunca executado |
| `linux.rs` | portal XDG — recusa com erro claro; falta consumir o nó do PipeWire |

O quadro sai da captura como **buffer de GPU** e vai direto para o encoder, sem cópia.

> **O som do próprio app fica de fora.** Quem filtra é o sistema operacional, por
> processo (`excludes_current_process_audio`), não um `if` no nosso código. Sem isso,
> compartilhar áudio devolveria a voz de quem está na chamada.

### `crates/media`

| Arquivo | Papel |
|---|---|
| `macos.rs` | VideoToolbox: tempo real, sem B-frames (menos latência), keyframe a cada 2 s |
| `audio.rs` | Opus em blocos exatos de 20 ms — a captura entrega pedaços de tamanho variável |
| `peer.rs` | `PeerLink`: uma conexão WebRTC direta, com trilha H.264 e Opus |
| `plain.rs` | `PlainSender`: RTP puro protegido por SRTP, para publicar no SFU |
| `lib.rs` | perfis de qualidade e o stub do encoder fora do macOS |

O stub fora do macOS tem a **mesma forma** do encoder real de propósito: sem ele o app
nem compilaria em Windows e Linux, e aí nem o `.msi` sairia.

### `apps/desktop`

| Arquivo | Papel |
|---|---|
| `src-tauri/src/lib.rs` | os comandos que a interface chama: transmitir, ofertar, aceitar resposta, subir para o SFU, atualizar |
| `src-tauri/src/settings.rs` | SQLite local: bind, monitor, iniciar com o sistema — o que é desta máquina |
| `src-tauri/wix/windows.wxs` | o que o instalador padrão não faz: liberar o app no firewall do Windows |
| `src-tauri/src/broadcast.rs` | junta captura, encoder e transporte: **um encoder alimenta N conexões** |
| `ui/app.js` | a aplicação: login, servidores, canais, voz, microfone |
| `ui/p2p.js` | as conexões diretas e a troca automática para o SFU |
| `ui/api.js` | cliente da API do Laravel |

---

## As três formas de a tela chegar do outro lado

É a decisão mais importante do projeto, e depende de quantas pessoas assistem.

| Quem transmite | Quantos assistem | Caminho | Por quê |
|---|---|---|---|
| Navegador | qualquer número | SFU | o navegador não tem encoder de hardware para nós; o servidor replica |
| App | 1 a 3 | **direto, máquina a máquina** | a VPS está nos EUA e as pessoas no Brasil: ~20 ms em vez de ~139 ms |
| App | 4 ou mais | SFU, por RTP puro | o upload de quem transmite multiplicava; agora sobe uma vez só |

A troca é automática. Quem entra como quarto espectador dispara a mudança: as conexões
diretas se fecham e a transmissão passa a subir uma única vez.

**Por que o app não fala WebRTC com o SFU.** Fala RTP puro no `PlainTransport` do
mediasoup: sem ICE e sem DTLS, porque o servidor já sabe o que vem — o lado Rust escolhe
SSRC, payload type e a chave SRTP e anuncia tudo **antes** do primeiro pacote. `comedia`
faz o servidor aprender o endereço de origem do primeiro pacote, então o app não precisa
ser alcançável de fora. SRTP não é opcional: sem ele a tela atravessaria a internet limpa.

**A topologia do P2P é assimétrica.** Quem envia é o Rust (captura nativa + encoder por
hardware); quem recebe é o **WebRTC do próprio webview**, num `<video>`. O que faltava no
WKWebView era só o `getDisplayMedia` — receber vídeo ele faz bem, e assim não é preciso
decodificar nem desenhar em Rust.

---

## O microfone

O mesmo código nos dois, importado e não copiado: `MicrophoneGate` decide quadro a quadro
se o áudio sai.

| Modo | Comportamento |
|---|---|
| Detecção de voz (padrão) | abre acima do limiar, com 300 ms de janela para não picotar palavra |
| Apertar para falar | só com a tecla segurada — lê `event.code`, funciona em qualquer layout |

Duas decisões que parecem detalhe e não são:

**Mute é o portão, nunca o producer.** Abrir e fechar producer a cada sílaba renegocia o
transporte dezenas de vezes por minuto, e refazer `getUserMedia` pisca o indicador de
microfone do sistema.

**O medidor roda na thread de áudio, não num timer.** Em janela em segundo plano o
navegador estrangula `setTimeout`: 60 ms viram 1000 ms. Um `AudioWorklet` entrega a cada
53 ms na mesma janela oculta. Sem isso, minimizar o app corta um segundo do início de
cada frase.

No app a voz passa pelo **SFU** (que replica para quantas pessoas forem) enquanto a tela
fica direta: áudio é barato, vídeo não.

---

## O que fica onde

| Guardado | Onde | Por quê |
|---|---|---|
| Conta, servidores, canais, mensagens | MySQL, no servidor | é o que precisa ser o mesmo em qualquer máquina |
| Token de sessão do app | `localStorage` do webview | é por dispositivo, e some se o app for reinstalado |
| Bind do apertar-para-falar, limiar do microfone, iniciar com o sistema | **SQLite local** (`settings.db`) | preso à máquina: atalho e monitor não fazem sentido viajando |

O SQLite fica no diretório de dados do app (`%APPDATA%/com.unkvoid.desktop` no Windows,
`~/Library/Application Support/…` no macOS). É SQLite e não um JSON solto porque o Rust
e o webview escrevem nele ao mesmo tempo: um arquivo reescrito inteiro a cada tecla
perde dados quando duas escritas se cruzam, e um desligamento no meio o deixa truncado.

---

## Portas

```
443/tcp    nginx  →  /       php-fpm (Laravel)
                  →  /sfu    127.0.0.1:3000  (sinalização, WebSocket)
                  →  /app    127.0.0.1:8081  (Reverb, WebSocket)
40000-40003/udp   WebRTC — uma porta por worker do mediasoup
41000-41031/udp   RTP puro do app desktop — 8 por worker
```

A mídia **não** passa pelo nginx: vai direto do navegador para as portas UDP. O mediasoup
é ICE Lite, ou seja só responde e nunca inicia — atrás de firewall stateful essas portas
precisam aceitar entrada não solicitada.

---

## O que verifica o quê

| Comando | Cobre |
|---|---|
| `cd web && php artisan test` | permissões, API do desktop, autorização do broadcast |
| `node web/resources/js/voice/MicrophoneGate.check.mjs` | detecção de voz, apertar-para-falar, mute, o portão |
| `cd sfu && pnpm run check` | o contrato inteiro do SFU, incluindo o RTP puro |
| `cd native && cargo test --workspace` | Opus em blocos de 20 ms, pacotização e SRTP em socket real |
| `cargo run -p media --example plain -- <ws> <token>` | o SFU **confirmando** que recebe o que o Rust manda |
| `.github/workflows/desktop.yml` | fmt, clippy e testes nos três sistemas; instaladores e `latest.json` |
