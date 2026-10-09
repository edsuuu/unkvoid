# Arquitetura

O mapa do Unkvoid: para que serve cada peça, como elas conversam e por onde passa cada coisa. O
detalhe de cada assunto mora num arquivo desta pasta, apontado em cada seção. O contrato do que
atravessa a rede é o [CONTRATO.md](CONTRATO.md).

## Para que o projeto existe

**Compartilhar a tela sem perder fps no jogo.** No navegador o encoder de vídeo roda na CPU, o
jogo e a compressão disputam o mesmo processador e a transmissão cai para 1 fps. O app usa o chip
de codificação da placa de vídeo:

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
 │  Interface nativa: Slint · GTK4 · SwiftUI            │
 │            ▲  crate (Windows, Linux) / ABI C (macOS) │
 │            │                         ▼               │
 │  Núcleo em Rust (shared/core)                        │
 │    regras, sessão, captura, encoder, RTP/SRTP        │
 └───┬──────────────────┬───────────────────────┬───────┘
     │ HTTPS            │ WSS                   │ UDP
     │ API, login,      │ sinalização e         │ mídia: RTP/SRTP
     │ atualização      │ tempo real, pelo SFU  │
 ┌───▼──────────────────▼───────────────────────▼───────┐
 │ VPS (unkvoid.com)                                    │
 │                                                      │
 │  nginx :443                                          │
 │   ├─ /  /api  /downloads          → Laravel (web/)   │
 │   ├─ /sfu  /health                → SFU :3000 (sfu/) │
 │   └─ /apt                         → APT, no MinIO    │
 │                                                      │
 │  SFU: Node (sinalização) + workers C++ (mídia)       │
 │       UDP 41000-42000 (RTP puro), sem nginx          │
 │                                                      │
 │  Docker: MySQL · MinIO (s3.unkvoid.com) · e-mail     │
 └──────────────────────────────────────────────────────┘
        Laravel ⇄ SFU: HTTP assinado, nos dois sentidos
```

## As três peças

| Pasta | Para que serve | Tecnologia | Roda em | Dona de | Nunca faz |
|---|---|---|---|---|---|
| `native/` | tudo o que acontece na máquina de quem usa: capturar, comprimir, mandar, receber e mostrar | núcleo em Rust; interface nativa por sistema (Slint, GTK4, SwiftUI) | Windows, Linux, macOS | interface, captura, encoder, mídia local | decidir permissão: só esconde botão |
| `sfu/` | relé de mídia: recebe cada transmissão uma vez e replica para quem assiste | Node 22 + mediasoup | VPS | salas, pessoas conectadas, producers, consumers, a mídia | decidir permissão: só confere a assinatura |
| `web/` | site, contas e tudo que precisa de banco | Laravel 13, Livewire 4, Flux, Sanctum | VPS | conta, servidor, cargo, canal, membro, mensagem, auditoria, versões do app | tocar em mídia |

Fora das três:

| Pasta | O que tem |
|---|---|
| `infra/` | nginx, `docker-compose.yml` (MySQL, MinIO, e-mail), sysctl de UDP, `deploy-web.sh`, instalação do runner |
| `.github/workflows/` | deploy do site e do SFU (pausados: só disparo à mão) e os builds do Tauri de antes (`release.yml`, `build-linux.yml`), que não se usam mais |

## `native/`: o app

O **núcleo** (`shared/core`) é Rust e decide tudo; a **interface** desenha e nada mais. As camadas,
o que é de cada sistema e o que fica no disco: [APP-NATIVO.md](APP-NATIVO.md).

```
    Rust (Windows, Slint)  ───┐
    Rust (Linux, GTK)      ───┼──► shared/core ──► capture · media · SFU · Laravel
    Swift (macOS)  ──► ABI C ─┘
```

**A regra que sustenta o desenho:** regra de negócio não mora em pasta de sistema. O teste é
direto: "o Windows vai precisar disto igual?" Se sim, sobe para o `core`.

**Por que interface nativa, e não uma webview** (decisão do dono, 20/09/2026): o app roda ao lado
de um jogo, e uma webview carrega um motor de browser inteiro para desenhar botão — e são três
motores diferentes (WebView2, WKWebView, WebKitGTK), o do Linux sem WebRTC. O preço é escrever a
tela três vezes; a lógica, que é o grosso, é escrita uma vez. O app Tauri + React de antes
(`native/apps/desktop`) continua no repositório como referência de comportamento.

### A ponte para o Swift

Uma ABI C pequena, em `shared/core/src/ffi.rs` — as ações, uma por uma, estão no
[CONTRATO.md](CONTRATO.md). **Só o macOS passa por ela.**

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

O `Handle` é seguro para uso concorrente: tudo atrás de `Mutex`, e as funções tomam `&`.

**Erro que chega à tela nunca carrega caminho, endereço nem código de status.** O núcleo devolve
um motivo (`unreachable`, `signedOut`, `notAllowed`, `gone`, `invalid`, `serverBroke`, `tooFast`,
e os `room.failed` da sala) e cada interface escreve a frase; o detalhe vai para o log. A única
exceção é erro de validação, que o Laravel já devolve em português e falando do campo. Ver
`shared/core/src/failure.rs`.

## `sfu/`: o relé de mídia

| Camada | O que faz | Onde roda |
|---|---|---|
| Sinalização | entrar, publicar, consumir, sair, tempo real: só troca mensagens | JavaScript, no processo Node |
| Mídia | mover pacote de vídeo e áudio | workers do mediasoup: processos C++ |

O Node não vê um único pacote de vídeo. Por que o SFU é Node: [DECISOES.md](DECISOES.md).

| Nome | O que é |
|---|---|
| `Room` | uma sala. O id é o código da sala por código (3 a 32 caracteres) ou o ULID de um canal de voz (26). A sala inteira mora num worker só, escolhido pelo que tem menos salas |
| `Peer` | uma pessoa conectada: um WebSocket, com uma `resumeKey` para voltar depois de cair |
| producer | uma origem que alguém manda: `screen`, `screenAudio`, `mic` ou `camera` |
| consumer | a cópia de um producer indo para uma pessoa; nasce pausado |
| transporte plain | RTP puro com SRTP: é por ele que o app nativo manda e recebe. Um por sentido por pessoa, a partir da porta 41000. O `comedia` aprende o endereço de quem manda pelo primeiro pacote (ninguém abre porta em casa); chave nova troca o transporte, e é assim que o app refaz o caminho quando o endereço muda |
| transporte WebRTC | o do app Tauri de antes. Uma porta por worker a partir de 40000, UDP com TCP de reserva |

As pastas, as rotas e as variáveis de ambiente: [`sfu/README.md`](../sfu/README.md).

## `web/`: o Laravel

| Frente | O que entrega |
|---|---|
| Site | página inicial com os downloads, cadastro, login (e-mail ou Google), recuperar senha, privacidade e termos |
| API do app (`/api`, Sanctum) | conta, servidores, cargos, canais, sobrescritas, membros, banimentos, mensagens, amigos, mensagens diretas, token de voz, token da sala por código, auditoria, `GET /api/config` |
| Tempo real (pelo SFU) | `channel.{ulid}` (mensagens e voz de um canal), `server.{id}` (quem está online e mudança de estrutura), `user.{id}` (amigos, DMs, expulsão), `releases` (versão nova). O socket não reentrega o que se perdeu numa queda: o app busca de novo pela API ao reconectar |
| Distribuição | `/downloads/latest.json` (manifesto da atualização), `/downloads/{sistema}`, `POST /api/releases` assinado |
| Registro sem tela | `POST /api/errors` (o log com `ERROR` dos apps) e `guest_accesses` (visitantes da sala por código) gravam no banco; não há tela de administração |
| Webhook do SFU | `POST /api/sfu/events`, assinado |

| Caminho | O que faz |
|---|---|
| `app/Http/Controllers/Api/` | um controller por recurso |
| `app/Http/Requests/`, `app/Http/Resources/` | validação da entrada e formato da resposta |
| `app/Http/Middleware/` | `VerifySfuSignature` e `VerifyReleaseSignature` |
| `app/Models/` | o modelo de dados (abaixo) |
| `app/Events/` | o que o Laravel publica no SFU |
| `app/Services/Sfu/SfuClient.php` | assina o token de voz e chama o SFU (`kick`, `mute`, `presence`, `broadcast`) |
| `app/Services/Storage/BucketService.php` | o bucket do MinIO |
| `app/Livewire/` | as páginas do site |
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
| Órfã | `clips`: os clipes do servidor saíram do código, e apagar a tabela é decisão do dono (migration) |

**Permissão** é um conjunto de bits calculado na ordem do Discord: dono ou `ADMINISTRATOR` pode
tudo; senão `@everyone` mais os cargos, depois as sobrescritas do canal (`@everyone`, cargos,
membro), e a hierarquia de cargos decide em quem se pode mexer. Canal sem `VIEW_CHANNEL` nem
aparece. A regra completa está no [CONTRATO.md](CONTRATO.md#permissões-bits-ubigint).

## Como as peças conversam

| De → para | Por onde | Quem prova quem é | Para quê |
|---|---|---|---|
| interface → núcleo | chamada direta (Windows, Linux) ou ABI C (macOS) | mesmo processo | tudo |
| app → Laravel | HTTPS, JSON | `Authorization: Bearer` do Sanctum | tudo do modo servidor |
| app → SFU | WSS em `/sfu` | token HMAC de 60 s assinado pelo Laravel, ou nenhum (sala por código) | sinalização |
| app → SFU | WSS, o mesmo socket | `identify` com o token de `POST /api/sfu/session`, e `subscribe` por canal | chat, presença e versão nova em tempo real |
| app ⇄ SFU | UDP | chave SRTP sorteada por transmissão | a mídia |
| Laravel → SFU | HTTP assinado (`x-unkvoid-timestamp`, `x-unkvoid-signature`) | `SFU_SECRET` | expulsar, mutar, presença, publicar no tempo real |
| SFU → Laravel | webhook `POST /api/sfu/events` assinado | `SFU_SECRET` | quem entrou e saiu |
| navegador → app | `127.0.0.1:<porta>` com `state` | `state` sorteado pelo app | voltar do login com Google |
| app → Laravel | `GET /downloads/latest.json`, `POST /api/errors` | nenhum (teto por IP) | atualização e log de erro |
| quem publica → Laravel | `POST /api/releases` assinado | `RELEASE_SECRET` | publicar uma versão |

O `SFU_SECRET` é o mesmo nos dois lados. Sem ele (ou com menos de 32 caracteres) o SFU não sobe:
é melhor fora do ar do que aberto.

## Os fluxos

### 1. Sala por código

1. A pessoa escreve o nome e clica em **Criar uma sala**. O núcleo sorteia 12 caracteres de
   `a-z0-9`.
2. O app confere `GET /health` antes de deixar entrar.
3. Abre o WebSocket e manda `join` **sem token**. O SFU cria a sala se ela não existir e dá à
   pessoa `guest:<installId>`, com tudo liberado. Quem está logado pede antes um token em
   `POST /api/rooms/{code}/token`, para valer a regra de uma sessão por conta.
4. O webhook `joined` vira uma linha em `guest_accesses`.
5. A sala some quando o último sai.

O que protege: o teto de conexões novas por IP, e o `join` sem token recusar código de 26
caracteres, que é o formato do id de canal de voz.

### 2. Entrar numa voz de servidor

1. O app pede `POST /api/channels/{id}/voice/token`. O Laravel confere `CONNECT` e o limite de
   pessoas e assina um token de 60 s com o que a pessoa pode produzir (`can`: `speak`, `stream`,
   `video`).
2. O app manda `join` com o token. O SFU confere a assinatura e o vencimento, e derruba qualquer
   outra sessão da mesma conta (`replaced`).
3. O SFU avisa o Laravel (`joined`); o Laravel registra em `channel_accesses` e emite
   `VoiceStateUpdated`.
4. O microfone entra **mutado**, em RTP puro (`producePlain`). Mutado, ele manda silêncio: sem
   pacote, o SFU derrubaria o producer em 30 s.
5. Se a sinalização cair, o app espera um tempo sorteado e volta, sem desistir enquanto a rede
   estiver fora (desiste só com o servidor recusando). Dentro de 30 s a sessão é retomada pela
   `resumeKey` sem derrubar a mídia; o caminho de chegada é refeito, porque o endereço da pessoa
   pode ter mudado. Depois da carência, entra de novo e republica o que transmitia.

### 3. Compartilhar a tela: o caminho do quadro

```
share          captura → textura na GPU → encoder de hardware (H.264, sem B-frames)
producePlain   o núcleo escolhe SSRC, tipo de payload e chave SRTP; o SFU devolve a porta e a chave dele
PlainSender    empacota RTP (MTU 1200), cifra SRTP e solta o vídeo no ritmo do pacer
SFU            aprende o endereço no primeiro pacote e replica para cada consumer
```

- O som da tela vai junto: Opus 48 kHz estéreo, no mesmo transporte, com SSRC próprio.
- Quadro-chave a cada 4 s no Windows e a cada 1 s no Linux (o `gst-launch` da captura não atende
  pedido de fora), mais os pedidos de quem assiste, espaçados de 2 a 4 s.
- Perda entre o app e o SFU: o SFU pede o pacote de volta (NACK por SRTCP) e o app reenvia do
  histórico, pelo pacer; se não der, pede quadro-chave (PLI).
- A taxa acompanha a perda: muito NACK numa janela e o governador baixa o alvo do encoder, até
  35% da taxa da qualidade; perda que continua no piso desce a resolução (720p, depois 720p30), e
  um minuto limpo sobe de volta. Por que não REMB: [DECISOES.md](DECISOES.md).
- Tela parada: o Windows codifica de novo a última imagem a cada 1 s, como o WebRTC.
- Vigias de quem transmite (`StallWatch`, no `room.rs`): captura ou encoder parado é refeito no
  mesmo producer; o encoder da placa que trava seguido cai para o de CPU; 5 s mandando sem nenhum
  RTCP do servidor é caminho morto, e tudo sobe de novo por outro socket.
- O SFU derruba a transmissão que passa 30 s sem pacote desde o começo, e avisa quem transmite
  (`producerDead`); o mesmo aviso, com `reason: 'revoked'`, sai quando a permissão cai.
- Taxa por qualidade: 720p60 a 5 Mb/s, 1080p60 a 10, 1440p60 a 20, 2160p60 a 40. Os ajustes de
  rede medidos: [REDE.md](REDE.md).

### 4. Assistir

1. `consumePlain` com a chave de chegada do app. Um `PlainReceiver` por sessão recebe tudo num
   socket só e separa por SSRC.
2. No vídeo, o que se perde é pedido de novo (NACK, e o SFU reenvia pelo RTX); buraco que não
   volta a tempo vira pedido de quadro-chave, repetido a cada 1 s enquanto a imagem não volta.
3. Cada quadro entra no decodificador no horário do relógio do RTP de quem transmite
   (`media::Playout`), e não na hora em que chegou: um bolo de quadros segurado por um reenvio sai
   espaçado.
4. Decodificar é de cada sistema: Media Foundation na placa no Windows, `gst-launch` no Linux,
   VideoToolbox no macOS. O som perdido é estimado pelo Opus (PLC/FEC).
5. O SFU diz se cada tela está chegando nele (`producerReceiving`). Recebendo lá e nada chegando
   aqui há 5 s, o caminho de chegada é refeito; tela parada de quem transmite não conta.
6. Janela fora da vista por 2 s: o vídeo é pausado no servidor e volta com quadro-chave. Abrem
   sozinhas 2 telas em PC de até 4 núcleos, 4 nos outros; as demais ficam no "Assistir".
7. O evento `watchers` diz quem está olhando cada tela.

### 5. Chat e tempo real

1. `POST /api/channels/{id}/messages`: o Laravel confere `SEND_MESSAGES` e grava. Até 3 imagens
   por mensagem, no bucket privado. Canal de voz também tem chat.
2. `MessageSent` vai para `channel.{ulid}`, e o SFU só deixa assinar quem tem `VIEW_CHANNEL`.
3. Mudou a estrutura do servidor: `ServerUpdated` no `server.{id}`, e o app refaz o `GET`.
4. Amigos, mensagens diretas e expulsão chegam no `user.{id}`.
5. O socket caiu e voltou: nada do que passou é reentregue, então o app se identifica de novo,
   reinscreve os canais e busca o que estava na tela.

### 6. Moderação: mutar, expulsar, banir

O Laravel decide, confere a hierarquia e grava. Depois chama o SFU pelo HTTP assinado: `mute`
pausa o producer do microfone (e a pessoa recebe `serverMuted`), `kick` fecha o socket.
`MemberRemoved` no `user.{id}` faz o app sair do servidor. Um token de voz emitido antes da
expulsão ainda vale 60 s, então o `joined` de quem já não é membro dispara um `kick` na hora.

### 7. Login com Google no app

O núcleo abre uma porta em `127.0.0.1`, manda a pessoa ao navegador (`GET /oauth2/app` com a porta
e um `state` sorteado), e o Laravel, depois do Google, devolve o navegador para essa porta com o
token do Sanctum. O `state` de volta tem de ser o que saiu (`shared/core/src/google.rs`).

### 8. Atualização e publicação

- Windows: o app lê o manifesto na abertura e, com ele aberto, ouve `ReleasePublished` no canal
  `releases`; baixa, confere a assinatura minisign e mostra o botão de atualizar. Quem instalou
  pela Microsoft Store atualiza pela Store.
- Linux: o `.deb` no repositório APT, assinado com GPG. Quem atualiza é o `apt`.
- Publicar é a partir da máquina de quem publica (o instalador no Windows, o `.deb` num Debian 12
  em contêiner), pelo `publish-release.sh` e pelo `apt-publish.sh`.

As duas chaves e o passo a passo: [AUTO-UPDATE.md](AUTO-UPDATE.md).

## Onde roda

### Produção: uma VPS

| Processo | Porta | Observação |
|---|---|---|
| nginx | 80, 443 | TLS; distribui por caminho (visão geral) e serve `s3.unkvoid.com` para o MinIO |
| Laravel (php-fpm 8.4) | atrás do nginx | cada deploy numa pasta nova; o `current` troca quando tudo está pronto |
| SFU | `127.0.0.1:3000` + UDP | pm2, 3 workers (núcleos menos um, numa VPS de 4) |
| MySQL 8.4 | `127.0.0.1:3307` | Docker |
| MinIO | `127.0.0.1:9000` | Docker; instaladores, fotos, ícones e o repositório APT |
| e-mail | as do e-mail | Docker (`docker-mailserver`) |
| runner do GitHub Actions | — | os deploys por disparo à mão |

Portas que o firewall do painel precisa abrir:

| Porta | Protocolo | Para quê |
|---|---|---|
| 80, 443 | TCP | site, API, WebSockets |
| 41000-42000 | UDP | RTP puro: quem transmite e quem assiste |
| 40000 até 40000 + workers − 1 | UDP e TCP | o WebRTC do app Tauri de antes (uma por worker) |

Porta fechada não dá erro: a transmissão "funciona" e ninguém vê nada. Por que a faixa é larga:
[UDP.md](UDP.md). A máquina e as medições: [SERVIDOR.md](SERVIDOR.md). Levantar do zero:
[INSTALAR-VPS.md](INSTALAR-VPS.md).

### Deploy

Os workflows de deploy estão pausados (só disparo à mão); hoje o deploy é na VPS, no clone
`/var/www/projects/unkvoid`:

| Peça | Como |
|---|---|
| site | `git merge --ff-only origin/<branch>` e `bash infra/deploy-web.sh`: release nova, migrations, recarrega o php-fpm |
| SFU | o mesmo clone, `pnpm install --frozen-lockfile --prod=false && pnpm run build` em `sfu/`, e `pm2 restart sfu` — quem está em chamada cai por alguns segundos (detalhes no [`sfu/README.md`](../sfu/README.md)) |
| apps | ver [AUTO-UPDATE.md](AUTO-UPDATE.md) |

### Local

Os comandos para subir as três peças e as verificações antes de entregar estão no
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Segurança, em uma linha por camada

| Camada | O que protege |
|---|---|
| Mídia | SRTP (AES-128 + HMAC-SHA1) com chave sorteada por transmissão |
| Entrada no SFU | token HMAC de 60 s com o que a pessoa pode produzir; o SFU recusa o resto |
| Chamadas entre Laravel e SFU | assinatura com o `SFU_SECRET` e janela de 300 s |
| Sala por código | o código é a única credencial; teto de conexões novas por IP |
| Atualização | assinatura minisign conferida pelo próprio app; APT assinado com GPG |
| Imagens e instaladores | bucket privado, toda URL assinada e com prazo |
| Token da conta | cifrado no disco, com a chave no chaveiro do sistema |

O que não está protegido, e por quê: [SEGURANCA.md](SEGURANCA.md).

## Glossário

| Termo | O que é |
|---|---|
| SFU | *Selective Forwarding Unit*: servidor que recebe a mídia de cada um e repassa para os outros sem decodificar |
| producer / consumer | no mediasoup, a mídia que entra de alguém / a cópia que sai para alguém |
| RTP puro (*plain*) | mídia em RTP direto sobre UDP, sem ICE nem DTLS: o app já combinou SSRC e chave com o servidor |
| SRTP | RTP cifrado e autenticado |
| SSRC | o número que identifica um fluxo dentro do RTP; um por origem |
| `comedia` | o servidor aprende o endereço de quem manda pelo primeiro pacote que chega |
| NACK / RTX | pedido de reenvio de pacote perdido / o fluxo pelo qual o reenvio volta |
| PLI | pedido de quadro-chave, mandado quando falta pacote |
| ULID | o id dos canais: 26 caracteres, ordenável por tempo |
| Sanctum | o token de API do Laravel que o app guarda |
