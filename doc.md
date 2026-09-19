# Como o Unkvoid funciona

Um mapa do projeto para quem chega agora: o que cada peça faz, como elas conversam e onde
mora cada coisa. Build, portas e armadilhas estão no [README.md](README.md); o contrato de
rede, com cada rota e evento, está em [docs/SERVIDORES.md](docs/SERVIDORES.md).

## A ideia em uma frase

Compartilhar a tela **sem perder fps no jogo**. No navegador o encoder de vídeo roda na CPU
e disputa com o jogo; aqui o app usa o chip de codificação da placa de vídeo:

```
captura → textura na GPU → encoder de hardware → 1 quadro → SFU → N espectadores
```

O quadro não desce para a memória da CPU antes de ser comprimido, é comprimido uma vez e
sobe uma vez. O SFU replica para quantos estiverem assistindo, então o upload de quem
transmite não cresce com a plateia.

## As três peças

```
            ┌──────────────────────── VPS ─────────────────────────┐
            │                                                      │
 app  ──────┼── HTTPS /api ──────► web/ (Laravel)  ── MySQL, MinIO │
 (native/)  │   wss Reverb  ◄────── chat, presença, eventos        │
            │                          │  ▲                        │
            │                   assina │  │ webhook                │
            │                    token │  │ (entrou/saiu)          │
            │                          ▼  │                        │
            └── wss /sfu + UDP ──► sfu/ (Node + mediasoup) ────────┘
                RTP da tela/voz        replica para N espectadores
```

| Peça | Faz | Nunca faz |
|---|---|---|
| `native/` — app (Rust + Tauri + React) | interface, captura, encoder, mídia local | decidir permissão (só esconde botão) |
| `sfu/` — relé de mídia (Node 22 + mediasoup) | salas, peers, producers, consumers | decidir permissão (só confere assinatura) |
| `web/` — site e API (Laravel 13 + Livewire 4 + Flux) | contas, servidores, cargos, canais, mensagens, auditoria | mídia |

O nginx da VPS (`infra/nginx-unkvoid.conf`) põe TLS na frente de tudo: `/api` e o site vão
para o Laravel, `/sfu` e `/health` para o SFU, `/app` e `/apps` para o Reverb.

## Os dois modos

### Sala por código (sem conta)

1. O app pede um nome e cria uma sala: sai um código de 12 caracteres.
2. O app abre um WebSocket com o SFU e entra como `guest:<installId>`. Sem token, sem
   Laravel no caminho.
3. Quem recebe o código entra na mesma sala e passa a consumir a tela de quem transmite.
4. O código **é** a sala: não existe em banco, some quando o último sai. O que impede
   varrer códigos é o teto de conexões novas por IP no SFU.

Isso é produto, não legado: mexer no modo com conta não pode piorar este.

### Servidores (com conta)

No molde do Discord: servidor, cargos com bits de permissão, canais de texto e voz,
sobrescritas por cargo e por membro (é assim que se oculta canal), convite, expulsar,
banir, chat, voz, câmera, amigos e mensagens diretas.

**Entrar num canal de voz:**

1. O app faz login na API (Sanctum) e recebe um Bearer.
2. `GET /api/config` diz onde estão o SFU e o Reverb.
3. Antes de cada entrada o app pede um token de voz ao Laravel. O Laravel calcula a
   permissão efetiva (na ordem do Discord) e assina um token de **60 s**:
   `base64url(json).hmac_sha256(SFU_SECRET)`, com `room`, `sub`, `exp` e `can`
   (`speak`, `stream`, `video`).
4. O app entrega o token ao SFU. O SFU só confere a assinatura e o `can`: sem `stream`,
   recusa produzir tela; sem `speak`, microfone; sem `video`, câmera.
5. O SFU avisa o Laravel por webhook assinado quem entrou e saiu; o Laravel repassa pelo
   Reverb (`VoiceStateUpdated`) para quem enxerga aquele canal.

**Chat:** mensagem vai por `POST` na API, o Laravel grava e transmite pelo Reverb
(`MessageSent` em `private-channel.{ulid}`). O app escuta com `laravel-echo`.

## O caminho do quadro no app

```
crates/capture          crates/media                         SFU
  ScreenCaptureKit  ─►  VideoToolbox / Media Foundation  ─►  RTP puro (producePlain)
  Graphics Capture      (NVENC, QuickSync, VCE) / x264       por UDP
  ximagesrc (X11)       Opus para o áudio
```

- **Transmitir** sai do Rust direto para o SFU por RTP puro, sem passar pelo webview.
- **Assistir** usa o WebRTC do webview. No Linux o WebKitGTK vem sem WebRTC, então o SFU
  manda RTP puro (`consumePlain`), o Rust abre o SRTP e o GStreamer decodifica
  (`crates/media/src/receiver.rs`).
- A interface React conversa com o Rust pelos comandos do Tauri (lista em
  `docs/SERVIDORES.md`, seção "App — comandos do Tauri").

## Estrutura de pastas

```
.
├── native/                      o app
│   ├── Cargo.toml               workspace Rust
│   ├── crates/
│   │   ├── capture/             captura de tela e áudio do sistema, um arquivo por SO
│   │   │                        (macos.rs, windows.rs, windows_audio.rs, linux.rs)
│   │   └── media/               encoder por hardware, Opus, RTP/SRTP para o SFU,
│   │                            receptor nativo do Linux; examples/ para testar isolado
│   ├── apps/desktop/
│   │   ├── src-tauri/           o processo Rust do app: comandos, janela, atalhos,
│   │   │                        login, transmissão (broadcast.rs), recepção (watch.rs)
│   │   ├── ui/                  interface React + TypeScript
│   │   │   ├── core/            a lógica sem tela: ApiClient, SfuClient, Voice, Chat,
│   │   │   │                    Hub, Permissions, Store, ponte com o Tauri…
│   │   │   ├── components/      as telas: entry (entrada), room (sala por código),
│   │   │   │                    hub (servidores, canais, chat), layout, common
│   │   │   └── dev/             ponte do Tauri fingida para rodar no navegador
│   │   ├── tests/               static/ (checks de idioma e de comentário), unit/,
│   │   │                        integration/ (Vitest)
│   │   ├── harness.html         o mesmo bundle no navegador, com console
│   │   └── release.mjs, *.sh    publicar release, repositório APT, build na VPS
│   └── tests/linux/             cenários do app em Linux limpo, via Docker
│
├── sfu/                         o relé de mídia
│   ├── src/
│   │   ├── server.ts, config.ts entrada e variáveis de ambiente
│   │   ├── Http/                rota → Request → Controller → Resource
│   │   │                        (join, transport, producer, consumer, peer, leave)
│   │   ├── Services/            Room, Peer, RoomRegistry, Signature (confere o token),
│   │   │                        Webhook (avisa o Laravel)
│   │   └── Enums/, Exceptions/
│   ├── check.mjs                o contrato inteiro contra um servidor no ar
│   ├── check-heartbeat.mjs      queda e volta de conexão
│   └── deploy.sh, install.sh, ecosystem.config.cjs (pm2)
│
├── web/                         Laravel: site e API
│   ├── app/
│   │   ├── Http/Controllers/Api/  um controller por recurso: Server, Channel, Role,
│   │   │                          Member, Ban, Overwrite, Message, Voice,
│   │   │                          Friend, DirectMessage, Room, Release, Config…
│   │   ├── Http/Controllers/Api/Sfu/  recebe o webhook do SFU
│   │   ├── Http/Middleware/     VerifySfuSignature, VerifyReleaseSignature
│   │   ├── Http/Requests/, Http/Resources/  validação de entrada e formato de saída
│   │   ├── Models/              User, Server, ServerRole, ServerMember, Channel,
│   │   │                        ChannelOverwrite, Message, Friendship, GuestAccess…
│   │   ├── Enums/               PermissionEnum (os bits), tipos de canal e mensagem…
│   │   ├── Events/              o que vai pelo Reverb (MessageSent, VoiceStateUpdated…)
│   │   ├── Services/Sfu/        cliente assinado que fala com o SFU (kick, mute, presença)
│   │   ├── Services/Storage/    bucket do MinIO
│   │   └── Livewire/            páginas do site: home, login/cadastro, senha
│   ├── routes/                  api.php (o app), web.php (o site), channels.php (Reverb)
│   ├── database/                migrations, seeders (cargos, admin), factory
│   ├── resources/views/         blades do site, e-mails, componentes Flux
│   └── tests/Feature/           Pest; checklist.html é o que foi validado à mão
│
├── infra/                       a VPS: nginx, docker-compose (MySQL + MinIO),
│                                sysctl de rede, deploy do site, runner do GitHub
├── .github/workflows/           deploy-web e deploy-sfu a cada push em main,
│                                build-linux, e release ao criar tag v*
├── docs/                        a documentação de verdade (tabela abaixo)
├── dist/                        instruções de instalação para quem recebe o .exe
├── Makefile, .run/              atalhos de dev, check e build por sistema
└── .claude/agents/              um agente especialista por peça (app, sfu, web)
```

## Onde procurar cada coisa

| Quero… | Vá em |
|---|---|
| ver uma rota, um evento, o formato do token | `docs/SERVIDORES.md` |
| entender por que algo foi feito assim | `docs/DECISOES.md` |
| saber o que ainda não rodou em hardware | `docs/ESTADO.md` |
| portas, firewall, UDP | `docs/REDE.md`, `docs/UDP.md` |
| subir uma VPS do zero | `docs/INSTALAR-VPS.md` |
| a VPS que existe hoje | `docs/SERVIDOR.md` |
| modelo de ameaça | `docs/SEGURANCA.md` |
| auto-update e build por sistema | `docs/AUTO-UPDATE.md`, `docs/BUILD-*.md` |
| rodar tudo local e verificar antes de entregar | `CLAUDE.md` |
