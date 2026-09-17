# Servidores, canais, voz e chat — o contrato entre as três peças

> Escrito em 11/09/2026. É a referência que Laravel (`web/`), SFU (`sfu/`) e app
> (`native/`) implementam. Mudou aqui, muda nos três. O plano e as regras de negócio
> completas estão em `~/.claude/plans/a-imagem-est-um-kind-sifakis.md`; este arquivo é
> o que atravessa a rede.

A sala anônima por código **continua como está**: quem não quer conta abre o app,
cria uma sala e manda o código. O que este documento descreve é o segundo modo, só
para quem está logado.

## Quem manda em quê

| Peça | Dono de | Nunca faz |
|---|---|---|
| Laravel | conta, servidor, cargo, canal, membro, mensagem, auditoria, quem pode o quê | mídia |
| SFU | `Room` (= canal de voz), `Peer`, producers, consumers | decidir permissão: só confere o token |
| App | interface, captura, encoder, mídia local | decidir permissão: só esconde botão |

## Permissões (bits, `ubigint`)

```
ADMINISTRATOR   = 1 << 0     VIEW_CHANNEL    = 1 << 8      MUTE_MEMBERS    = 1 << 15
MANAGE_SERVER   = 1 << 1     SEND_MESSAGES   = 1 << 9      DEAFEN_MEMBERS  = 1 << 16
MANAGE_ROLES    = 1 << 2     MANAGE_MESSAGES = 1 << 10     MOVE_MEMBERS    = 1 << 17   (desconectar da voz)
MANAGE_CHANNELS = 1 << 3     CONNECT         = 1 << 11
KICK_MEMBERS    = 1 << 4     SPEAK           = 1 << 12
BAN_MEMBERS     = 1 << 5     STREAM          = 1 << 13     (compartilhar tela)
CREATE_INVITE   = 1 << 6     VIDEO           = 1 << 14     (câmera)
VIEW_AUDIT_LOG  = 1 << 7
```

`@everyone` nasce com `VIEW_CHANNEL | SEND_MESSAGES | CONNECT | SPEAK | STREAM | VIDEO | CREATE_INVITE`.

Cálculo efetivo para um membro num canal (igual ao Discord):

1. Dono do servidor ou `ADMINISTRATOR` em qualquer cargo → tudo.
2. `base = @everyone.permissions | OR(cargos do membro)`.
3. Sobrescritas do canal, nesta ordem: `@everyone` (`base &= ~deny; base |= allow`),
   depois **todos os cargos do membro agregados** (`deny` de todos, depois `allow` de
   todos), depois a sobrescrita do próprio membro.
4. Hierarquia: `top(membro) = max(position)` dos cargos; `@everyone` tem `position = 0`.
   Só se mexe (expulsar, banir, dar/tirar cargo, mutar, desconectar) em quem tem `top`
   **menor** que o seu. Dono é infinito. Só se cria/edita/atribui cargo com `position`
   menor que o seu `top`.

## Token de entrada no SFU (Laravel → app → SFU)

`base64url(json) + "." + hex(hmac_sha256(base64url(json), SFU_SECRET))`, como já faz
`sfu/src/Services/Signature.ts`. Claims:

```json
{ "room": "01j7q0abcdefghijklmnopqrst", "sub": "user:12", "name": "Edsu", "exp": 1757640000,
  "can": ["speak", "stream", "video"] }
```

- `room` é o ULID do canal **em minúsculas** (26 chars de `a-z0-9`; passa no regex atual).
- `sub` é `user:<id>`. A sala anônima continua entrando sem token como `guest:<installId>`
  e o SFU dá a ela `can: ["speak", "stream", "video"]`.
- `exp` = agora + 60 s. O app pede um token novo **antes de cada `join`**, inclusive nas
  reconexões.
- `can` substitui o `owner: boolean` de hoje. O SFU recusa (403) `produce`/`producePlain`
  de `mic` sem `speak`, de `screen`/`screenAudio` sem `stream`, de `camera` sem `video`.

## SFU — o que muda no protocolo

Ações novas (mesmo envelope `{id, action, data}`):

| ação | data | resposta |
|---|---|---|
| `produce` | `{ transportId, kind, source, rtpParameters }` (WebRTC, `sendTransport` do mediasoup-client) | `{ producerId, kind, source }` |
| `pauseProducer` | `{ producerId }` | `{ status: 'paused' }` |
| `resumeProducer` | `{ producerId }` | `{ status: 'resumed' }` |
| `closeConsumer` | `{ consumerId }` | `{ status: 'closed' }` |

`Source` passa a ser `screen | screenAudio | mic | camera`. `producePlain` aceita os
quatro. `newProducer`, `producerClosed`, `consume`, `consumePlain` e `describePeers`
já carregam `source`; nada muda no formato deles. Os eventos ganham
`producerPaused { peerId, producerId }` e `producerResumed { peerId, producerId }`
para a sala inteira menos o dono.

Plateia: o SFU manda para a sala inteira `watchers { producerId, watchers: [{ peerId, name }] }`
— quem está **olhando** aquela transmissão agora, WebRTC e RTP puro no mesmo balde. Três
regras que o cliente precisa saber para não contar errado:

- **só `source: screen`**. Câmera e microfone ficam de fora: numa sala cheia é todo mundo
  consumindo todo mundo, e o evento viraria enxurrada;
- **nascer não é assistir**. O consumer nasce pausado, então a plateia só muda no
  `resumeConsumer` e no `pauseConsumer` — e em `closeConsumer`, na queda do transporte, na
  perda de sinalização (a pessoa sai da lista na hora) e na retomada dentro da carência (ela
  volta);
- quem está na carência de reconexão não conta como plateia.

`JoinResource` devolve `can: string[]` no lugar de `owner`. O app usa esse `can` (e não só
os bits do canal) para decidir se liga o mic, a câmera e a tela: mutado pelo servidor
chega sem `speak`. O SFU também manda `serverMuted { muted }` para a própria pessoa
quando o Laravel chama `/mute`, e recusa `resumeProducer` do mic enquanto durar.

**Uma conta, uma sessão no SFU inteiro.** O `join` com token (`sub` que não começa com
`guest:`) derruba qualquer outra sessão daquela conta, na mesma sala ou em outra, e ela
recebe `replaced { reason }` e perde o socket com o código 4002. A sala vê o `peerLeft` e
o Laravel o `left` da sala antiga. O visitante (`guest:`) só é substituído pela
`resumeKey`: o `installId` é escolhido pelo próprio app e a sala inteira o recebe no
`peerJoined`, então valer como identidade deixaria qualquer um derrubar qualquer um. O
app ignora `replaced`, `kicked` e `closed` de um `SfuClient` que já não é o atual.

O `join` sem token (sala anônima) recusa sala de 26 caracteres: é o formato do ULID de
canal, e sem isso qualquer um entraria num canal de voz sem passar pelo Laravel.

HTTP assinado (cabeçalhos `x-unkvoid-timestamp` e `x-unkvoid-signature`, assinatura
sobre `ts\nMÉTODO\ncaminho\ncorpo`, janela de 300 s — como o `kick` de hoje):

| rota | corpo | resposta |
|---|---|---|
| `POST /rooms/:code/kick` (já existe) | `{ "userId": "user:12" }` | `{ kicked: n }` |
| `POST /rooms/:code/mute` | `{ "userId": "user:12", "muted": true }` — pausa/retoma o producer `mic` daquela conta | `{ muted: n }` |
| `GET /presence` | corpo vazio | `{ rooms: { "<room>": [ { sub, name, sources: ["mic","screen"] } ] } }` |

`consumePlain` devolve também `ssrc` do consumer: o receptor nativo do Linux separa os
producers de uma mesma porta por SSRC, sem adivinhar pelo primeiro pacote.

Webhook do SFU para o Laravel, **fora do caminho do `join`**, fire-and-forget, para conta
(`user:`) e visitante da sala por código (`guest:<installId>`, `room` com o código de 3 a
32 caracteres). O visitante só vira linha em `guest_accesses` (nome, sala, IP, instalação,
entrada e saída), na aba "Visitantes" de `/admin/auditoria`: não há canal nem conta a
avisar. O SFU troca `installId` fora de `[A-Za-z0-9-]{1,64}` por um UUID sorteado.
`POST {SFU_LARAVEL_URL}/api/sfu/events` com os mesmos cabeçalhos assinados:

```json
{ "event": "joined", "room": "01j7…", "sub": "user:12", "name": "Edsu", "ip": "203.0.113.9", "at": 1757640000 }
{ "event": "left",   "room": "01j7…", "sub": "user:12", "name": "Edsu", "ip": "203.0.113.9", "at": 1757640090 }
```

`joined` dispara no `join` novo (não no `resume`); `left` dispara no `removePeer` real
(saída, expulsão, ou o fim dos 30 s de graça). Env novo: `SFU_LARAVEL_URL`
(`http://127.0.0.1:8000` local, `https://unkvoid.com` na VPS).

SSRC no RTP puro (app nativo): um por **origem**, não por tipo. `screen` e `camera`
são os dois `video`; sem SSRC distinto o mediasoup mistura.

## Laravel — API (`auth:sanctum`, JSON)

Tudo devolve `Resource`. Erro de permissão é 403 com `{ "message": "…" }`; validação é
422 no formato padrão do Laravel; hierarquia recusada é 403.

`GET /api/config` (público):
```json
{ "sfu": "ws://127.0.0.1:3000/sfu", "reverb": { "host": "127.0.0.1", "port": 8080, "key": "…", "scheme": "http" } }
```

Servidores:

| rota | corpo | resposta |
|---|---|---|
| `GET /api/servers` | — | `[ { id, name, owner_id, icon_url, last_accessed_at } ]`, do último acesso à voz mais recente para o mais antigo; nunca acessado vai para o fim, pela data em que entrou no servidor |
| `POST /api/servers` | `{ name }` | `ServerResource` (cria `@everyone`, `#geral` texto, `Geral` voz) |
| `GET /api/servers/{server}` | — | ver "árvore" abaixo |
| `PATCH /api/servers/{server}` | `{ name }` | `ServerResource` (`MANAGE_SERVER`) |
| `DELETE /api/servers/{server}` | — | 204 (dono) |
| `POST /api/servers/{server}/invite` | — | `{ invite_code }` (regenera; `CREATE_INVITE`) |
| `POST /api/invites/{code}` | — | `ServerResource` (entra; banido → 403) |
| `POST /api/servers/{server}/leave` | — | 204 (dono → 403) |
| `POST /api/servers/{server}/icon` | `multipart`, campo `icon` (jpeg/png/webp, ≤ 2 MB) | `ServerResource` (`MANAGE_SERVER`); guarda no mesmo bucket privado dos clipes e apaga o arquivo antigo |
| `DELETE /api/servers/{server}/icon` | — | 204 (`MANAGE_SERVER`); volta ao ícone padrão |
| `GET /api/servers/{server}/audits` | — | as 50 entradas mais recentes do histórico do servidor (`VIEW_AUDIT_LOG`) |

`icon_url` é pré-assinada e vence em 2 h, como a miniatura do clipe; sem ícone vem `null`.

Auditoria (`GET /api/servers/{server}/audits`), do mais recente para o mais antigo, com o
`meta`/`links` de paginação do Laravel:
```json
{ "id": "a12", "at": "…", "event": "deleted", "type": "Channel",
  "actor": { "id": 12, "name": "Edsu" }, "summary": "apagou o canal #geral" }
```
`id` leva a letra da fonte (`a` = tabela `audits` do pacote, `c` = `channel_audits`, que
existe porque o id do canal é um ULID e a coluna da `audits` é numérica). `event` é o do
pacote (`created`, `updated`, `deleted`, `sync`), `type` é o nome curto do modelo
(`Server`, `ServerRole`, `ServerMember`, `Channel`, `Message`), `actor` vem `null` se a
conta foi apagada, e `summary` já sai em português montado pelo Laravel. Entram o
servidor, seus cargos, seus membros, seus canais e as mensagens dos canais dele; cargo e
membro apagados de vez saem da lista, porque o filtro é por id que ainda existe.

Árvore (`GET /api/servers/{server}`):
```json
{ "id": 1, "name": "Meu servidor", "owner_id": 12, "icon_url": null, "invite_code": "abcdef1234" (só com CREATE_INVITE, senão null),
  "me": { "user_id": 12, "permissions": 262143, "top_position": 3 },   // dono: top_position = 2147483647
  "roles": [ { "id": 1, "name": "@everyone", "color": null, "position": 0, "permissions": 31552, "is_everyone": true } ],
  "channels": [ { "id": "01j7…", "name": "geral", "type": "text", "topic": null, "position": 0, "user_limit": null,
                  "permissions": 31552,   // as MINHAS efetivas neste canal
                  "overwrites": [ { "target_type": "role", "target_id": 1, "allow": 0, "deny": 256 } ] } ],   // só com MANAGE_ROLES
  "members": [ { "user_id": 12, "name": "Edsu", "avatar_url": null, "nickname": null, "role_ids": [1, 3],
                 "server_mute": false, "server_deaf": false, "is_owner": true } ],
  "voice": { "01j7…": [ { "user_id": 12, "name": "Edsu", "sources": ["mic"] } ] },   // do SFU (/presence), cache 3 s
  "bans": [ { "user_id": 40, "name": "Fulano", "reason": "…", "banned_by": 12, "created_at": "…" } ] }   // só com BAN_MEMBERS; banned_by null se quem baniu apagou a conta
```
Canais que eu não tenho `VIEW_CHANNEL` **não aparecem**.

Membros (`{user}` é id de usuário):

| rota | corpo | regra |
|---|---|---|
| `PATCH /api/servers/{server}/members/{user}` | `{ nickname?, role_ids?, server_mute? }` | `MANAGE_ROLES` para cargos (só cargos abaixo do meu top, e só com permissões que eu tenho), `MUTE_MEMBERS` para o bool (chama `POST /rooms/:code/mute` no SFU se a pessoa estiver em voz), apelido próprio sempre. `server_deaf` existe na tabela mas ainda não tem escrita |
| `DELETE /api/servers/{server}/members/{user}` | — | `KICK_MEMBERS` + hierarquia; derruba da voz via `kick` |
| `GET /api/servers/{server}/bans` | — | `BAN_MEMBERS` |
| `POST /api/servers/{server}/bans/{user}` | `{ reason? }` | `BAN_MEMBERS` + hierarquia; remove membro, derruba da voz |
| `DELETE /api/servers/{server}/bans/{user}` | — | `BAN_MEMBERS` |

Cargos:

| rota | corpo |
|---|---|
| `POST /api/servers/{server}/roles` | `{ name, color?, permissions }` (`MANAGE_ROLES`; posição = top do criador − 1, mínimo 1) |
| `PATCH /api/roles/{role}` | `{ name?, color?, permissions?, position? }` (`@everyone`: só `permissions`) |
| `DELETE /api/roles/{role}` | — (`@everyone` → 422) |

Canais:

| rota | corpo |
|---|---|
| `POST /api/servers/{server}/channels` | `{ name, type, topic?, user_limit? }` (`MANAGE_CHANNELS`) |
| `PATCH /api/channels/{channel}` | `{ name?, topic?, position?, user_limit? }` |
| `DELETE /api/channels/{channel}` | — (último canal de texto → 422) |
| `PUT /api/channels/{channel}/overwrites/{type}/{id}` | `{ allow, deny }` (`MANAGE_ROLES`; `type` = `role`\|`member`; `id` = id do cargo ou id do usuário) |
| `DELETE /api/channels/{channel}/overwrites/{type}/{id}` | — |

Mensagens:

| rota | corpo | resposta |
|---|---|---|
| `GET /api/channels/{channel}/messages?before={id}` | — | 50 mais recentes antes de `before`, ordem crescente: `[ { id, channel_id, user: {id,name,avatar_url}, type, body, reply_to, edited_at, created_at } ]` |
| `POST /api/channels/{channel}/messages` | `{ body }` (1–2000), `reply_to_id` opcional | `MessageResource` (`SEND_MESSAGES`). O `reply_to_id` tem de ser de mensagem **do mesmo canal**, senão 422: aceitar id de fora vazaria texto de canal que a pessoa talvez nem enxergue |
| `PATCH /api/messages/{message}` | `{ body }` | só o autor |
| `DELETE /api/messages/{message}` | — | autor ou `MANAGE_MESSAGES` |

`reply_to` é `null` ou `{ id, name, body }` com o corpo cortado em 120 caracteres — é só o
que o cartão da resposta mostra. Apagar a mensagem original é soft delete: a resposta
continua no ar e o `reply_to` dela passa a vir `null`, ou seja, a citação some da tela.

`type` é `user` (o normal) ou `join`. O aviso de chegada: entrar por convite grava, no
primeiro canal de texto por `position` que quem chegou enxerga, uma mensagem
`type: "join"` com `user` = quem entrou e `body` "chegou no servidor!" (que existe só para
o app antigo, que não conhece `type`, não mostrar balão vazio), e dispara o mesmo
`MessageSent`.
Quem já era membro e clicou no convite de novo não avisa de novo. A frase ("fulano chegou
no servidor") quem monta é o app: o Laravel não manda texto pronto.

Voz:

| rota | corpo | resposta |
|---|---|---|
| `POST /api/channels/{channel}/voice/token` | — | `{ token, url, expires_in: 60 }` (`CONNECT` no canal de voz; `user_limit` cheio → 403; grava `channel_accesses`) |
| `DELETE /api/channels/{channel}/voice/members/{user}` | — | 204 (`MOVE_MEMBERS` + hierarquia; `kick` no SFU) |

Webhook (assinado, sem Sanctum): `POST /api/sfu/events` — corpo acima. `joined` fecha
qualquer acesso aberto do mesmo usuário no mesmo canal e abre um novo (com `sfu_ip`);
`left` fecha o aberto. Os dois retransmitem `VoiceStateUpdated`.

## Amigos

Uma linha por par, na direção em que o pedido foi feito, com `status` `pending`,
`accepted` ou `blocked`. Bloquear não cria uma segunda linha: é a mesma mudando de
situação, e quem bloqueou passa a ser o `requester`.

| rota | corpo | resposta |
|---|---|---|
| `GET /api/friends` | — | `[FriendResource]` — os dois lados vão no recurso, porque a interface precisa saber se mostra "aceitar" ou "aguardando" |
| `POST /api/friends` | `{ email }` | `FriendResource` (throttle 20/min). E-mail que não existe responde o mesmo que e-mail não encontrado, para a busca não virar lista de quem tem conta |
| `PATCH /api/friends/{friendship}` | `{ action: accept\|block }` | `FriendResource`. Só quem recebeu aceita, e **linha bloqueada não aceita** |
| `DELETE /api/friends/{friendship}` | — | `204`. Recusar, desfazer e desbloquear são a mesma coisa — a linha some —, mas **linha bloqueada só quem bloqueou apaga** |

## Mensagens diretas

Conversa de duas pessoas, sem servidor no meio. Não existe tabela de conversa: o par já
identifica o fio.

**Quem pode conversar:** amizade em `accepted`, **ou** um servidor em comum. O servidor em
comum existe porque a ficha de perfil de um membro tem campo de mensagem — exigir amizade
ali daria 403 em todo mundo que ainda não é amigo, que é justamente quem se quer chamar.
**Bloqueio vence os dois** e fecha a conversa dos dois lados; quem foi bloqueado não
desfaz o próprio bloqueio (nem aceitando, nem apagando a linha). Sem nenhuma das duas
condições, mandar e ler dão 403. O que já foi dito continua no banco; some da tela de quem
desfez, e o par bloqueado some também da lista de conversas.

| rota | corpo | resposta |
|---|---|---|
| `GET /api/dm` | — | uma linha por conversa, a da mensagem mais recente primeiro: `[ { user: {id,name,avatar_url}, last: { id, body, created_at, mine }, unread } ]` |
| `GET /api/dm/{user}?before={id}` | — | as 50 mais recentes antes de `before`, ordem crescente: `[DirectMessageResource]`. **Marca como lidas** as que chegaram para quem pediu: é assim que o app zera o `unread` |
| `POST /api/dm/{user}` | `{ body }` (1–2000) | `DirectMessageResource` (throttle 60/min) |
| `POST /api/dm/{user}/read` | — | `204`. Marca como lidas as que chegaram daquela pessoa — é o que o app chama quando a mensagem cai com a conversa **já aberta**, porque aí não houve `GET` para marcar |
| `PATCH /api/dm/{directMessage}` | `{ body }` | só o autor; grava `edited_at` |
| `DELETE /api/dm/{directMessage}` | — | 204, só o autor (soft delete: some da conversa, fica no banco) |

```json
{ "id": 12, "body": "oi", "created_at": "…", "edited_at": null, "mine": true,
  "sender": { "id": 2, "name": "Edsu", "avatar_url": null } }
```

`mine` só existe na resposta HTTP, onde o dono da resposta é um só. No tempo real o pacote
é o mesmo para os dois lados, então ele **não leva `mine`** e leva `recipient`: quem recebe
faz `mine = message.sender.id === euId` e `pessoa = mine ? recipient : message.sender`.

## Clipes

Só no modo servidor. A sala por código não clipa e não muda em nada.

- O SFU guarda em anel os **últimos 5 minutos** de cada pessoa logada que compartilha
  tela num canal de voz: o vídeo `screen` (copiado, sem recomprimir), o `screenAudio` e o
  `mic` **dessa mesma pessoa**. A voz de mais ninguém da chamada entra. Parou de
  compartilhar ou saiu da sala, o anel dela é apagado.
- **Nada é guardado sem clique.** O que não foi clipado é sobrescrito. Clipar copia o que
  o anel tem naquele instante (até 5 min), e só isso vai para o MinIO.
- Quem clipa: quem está **dentro** daquele canal de voz agora (o SFU confere) e tem
  `VIEW_CHANNEL` + `CONNECT` nele (o Laravel confere). Pode clipar a própria tela.
- Quem vê: **só quem clipou**. Clipe de outra pessoa responde 404, nunca 403.
- Expira em **7 dias**: some da lista e da API (404), e o Laravel apaga a linha e
  `clips/{id}/` do MinIO. Sem teto de quantidade.
- Formato: HLS VOD em `clips/{clip_id}/` no bucket privado — `index.m3u8`, `seg-NNN.ts`
  (H.264 copiado + AAC 128 kbit/s com `screenAudio` e `mic` misturados; sem nenhum dos
  dois, só vídeo), `thumb.jpg` e `clip.mp4` (o mesmo conteúdo num arquivo só, para baixar).
- **Nenhuma URL de clipe é pública.** Bucket privado; miniatura, playlist, segmentos e
  download saem sempre assinados e vencem.

API (`auth:sanctum`, menos a playlist):

| rota | corpo | resposta |
|---|---|---|
| `POST /api/channels/{channel}/clips` | `{ user_id }` (quem está transmitindo) | 202 `ClipResource` com `status: processing`. Canal de texto → 422; sem `VIEW_CHANNEL`+`CONNECT` → 403; SFU diz que a pessoa não transmite → 422; SFU diz que eu não estou na voz → 403; SFU fora → 503. Recusado, a linha não fica |
| `GET /api/clips` | — | os meus, mais novo primeiro: `[ClipResource]` |
| `GET /api/clips/{clip}` | — | `ClipResource` (não é meu → 404) |
| `DELETE /api/clips/{clip}` | — | 204, e apaga `clips/{id}/` do MinIO (não é meu → 404) |
| `GET /api/clips/{clip}/playlist.m3u8` | URL assinada pelo Laravel (`temporarySignedRoute`, 2 h), sem Sanctum — o player não manda cabeçalho | `application/vnd.apple.mpegurl`: o `index.m3u8` do MinIO com cada segmento trocado por URL pré-assinada do MinIO (2 h) |

```json
{ "id": "01j8…", "status": "processing", "streamer": { "id": 40, "name": "Fulano" },
  "server_name": "Meu servidor", "channel_name": "Geral", "duration_ms": 300000, "size_bytes": 187000000,
  "created_at": "…", "expires_at": "…", "thumbnail_url": null, "playlist_url": null, "download_url": null }
```

`thumbnail_url` e `download_url` (pré-assinadas do MinIO, 2 h; a de download com
`Content-Disposition: attachment`) e `playlist_url` só vêm com `status: ready`. Os
nomes são cópia do momento do clipe: sobrevivem a servidor, canal ou conta apagados.

SFU, HTTP assinado (os mesmos cabeçalhos do `kick`):

| rota | corpo | resposta |
|---|---|---|
| `POST /rooms/:code/clips` | `{ "clipId": "01j8…", "clipper": "user:12", "streamer": "user:40", "upload": { "url": "…", "fields": { … }, "prefix": "clips/01j8…/" } }` | 202 `{ accepted: true }` · 404 `streamer` sem anel nesta sala · 403 `clipper` fora da sala · 503 sem `ffmpeg` |

`upload` é uma política de POST do S3 (`PostObjectV4`) que o Laravel assina, presa a
`starts-with $key clips/{id}/` e válida por 30 min. O SFU sobe cada arquivo com
`multipart/form-data` e nunca tem credencial do MinIO. O corpo leva **só** os `fields`, a
`key` (`prefix` + nome do arquivo) e o `file`, com a `key` antes do `file`: qualquer campo a
mais (até `Content-Type`) o MinIO recusa com 403. `upload.url` é `{endpoint}/{bucket}`.
O Laravel pode repetir o mesmo `clipId` depois de um timeout: o SFU aceita de novo (202)
sem gerar o clipe duas vezes.

Webhook (o mesmo `POST /api/sfu/events`, os mesmos cabeçalhos):

```json
{ "event": "clip.ready",  "clipId": "01j8…", "durationMs": 300000, "sizeBytes": 187000000, "at": 1757640000 }
{ "event": "clip.failed", "clipId": "01j8…", "reason": "…", "at": 1757640000 }
```

Env novo no SFU: `SFU_FFMPEG` (padrão `ffmpeg`) e `SFU_RECORDINGS_DIR` (padrão: a pasta
temporária do sistema).

## Reverb (tempo real)

Auth: `POST /broadcasting/auth` com `Authorization: Bearer <sanctum>`; o app usa
`laravel-echo` + `pusher-js` com `authEndpoint` apontando para `{SERVER}/broadcasting/auth`.

| canal | quem entra | eventos |
|---|---|---|
| `private-channel.{ulid}` | `VIEW_CHANNEL` | texto: `MessageSent { message }`, `MessageUpdated { message }`, `MessageDeleted { id, channel_id }` · voz: `VoiceStateUpdated { channel_id, user_id, name, event: joined\|left }` (no canal privado da própria voz, para canal oculto não vazar quem está nele; o app assina o canal privado de cada voz que enxerga) |
| `presence-server.{id}` | membro | (presença: `{ id, name, avatar_url }`) · `ServerUpdated { server_id }` (qualquer mudança de estrutura: o app refaz o `GET`) |
| `private-user.{id}` | o próprio | `FriendshipUpdated { friendship, removed }` (`FriendResource`, nos canais dos **dois** lados) · `MemberRemoved { server_id, reason: kicked\|banned }` · `ClipUpdated { clip }` (`ClipResource`, quando fica `ready` ou `failed`) · `DirectMessageCreated { message, recipient }`, `DirectMessageUpdated { message, recipient }`, `DirectMessageDeleted { id }` (nos canais dos **dois** lados da conversa; `message` é o `DirectMessageResource` sem o `mine`) |

### Expulsar e banir cortam a pessoa de tudo

Vale a partir do momento em que acontece; quem já tinha saído antes não é reprocessado.

- **Voz:** o Laravel chama o `kick` do SFU procurando a pessoa na presença fresca (sem o cache
  de 3 s). O token de voz de antes do kick vale 60 s: o webhook `joined` de quem já não é
  membro chama o `kick` na hora, sem abrir acesso nem emitir `VoiceStateUpdated`.
- **Tempo real:** `MemberRemoved` no `private-user.{id}` faz o app sair dos canais do servidor,
  e assinar de novo é recusado pela autorização do canal. **Limite:** o Reverb 1.11 não derruba
  a assinatura de quem já estava inscrito (não tem `pusher:signin`, então o
  `terminate_connections` não acha a conexão). Um cliente modificado que ignore o
  `MemberRemoved` continua recebendo os eventos dos canais que já assinava até reconectar.

## App — o que aparece

- Duas abas no topo: **Transmissão** (tudo o que está abaixo) e **Clipes**.
- Entrada: a tela de código continua; ao lado, "Entrar" (e-mail/senha ou Google pelo
  `/oauth2/app?port=` que já existe) e "Criar conta". Token do Sanctum em `localStorage`
  (`unkvoid:token`). Com token válido (`GET /api/me`), abre o modo servidor.
- Logado e sem servidor aberto, o centro mostra **Criar sala** e **Últimas salas**. Criar
  sala é `POST /api/servers { name }` (servidor com `#geral` e `Geral` de voz): abre o
  servidor, **não** entra na voz, e mostra o código de convite para mandar. Últimas salas
  é o `GET /api/servers`: abrir uma mostra quem está em cada voz, e entrar é um clique. A
  sala por código sem login continua como está.
- Entrar numa voz liga o microfone **mutado**; desmutar é da pessoa.
- Modo servidor: trilho de servidores | canais (texto e voz, quem está em cada voz) |
  centro (chat ou palco) | membros com cargos. Barra de voz embaixo: mutar, ensurdecer,
  câmera, **compartilhar tela (só aqui)**, **Clipar** (só quando alguém no canal
  compartilha tela: abre a lista de quem transmite), sair.
- Windows/macOS: mic e câmera pelo `getUserMedia` + `sendTransport.produce`. Linux:
  pelo Rust (`pulsesrc`/`v4l2src` → RTP puro), como a tela.
- Áudio de `screenAudio` chega **mudo**. `mic` toca direto. `camera` vira cartão pequeno.
- Aba Clipes sem login: o mesmo painel de entrar. Com login: os meus clipes (miniatura,
  quem transmitia, servidor e canal, data, duração); `processing` com indicador, `failed`
  com aviso; **Assistir** abre um mini player na própria aba (hls.js; HLS nativo onde
  existir); **Baixar** usa o `download_url`; **Apagar** pede confirmação.

## App — comandos do Tauri

A interface chama com `invoke`, com os argumentos em camelCase (`serverKey`, `producerId`); o
Tauri converte para o snake_case do Rust. Mudou um comando, mude aqui e em `ui/core`.

| Comando | Argumentos | Devolve | Para quê |
|---|---|---|---|
| `app_version` | — | `string` | versão instalada |
| `check_update` | — | versão nova ou `null` | procura, baixa e instala; emite `update:progress` com `[baixado, total]` |
| `restart` | — | — | reinicia depois de atualizar |
| `expand_window` | — | — | a janela nasce do tamanho de um diálogo e cresce quando o app está pronto |
| `log_line` / `log_path` | `line` / — | — / caminho | log em disco: a janela não tem console |
| `report_check` | `webrtc, receiver[], sender[], userAgent` | — | o diagnóstico do `--check`, chamado pela página que o próprio Rust abre |
| `list_displays` | — | `[{id, width, height}]` | as telas do seletor |
| `list_windows` | — | `[{id, title, application}]` | os aplicativos do seletor |
| `source_preview` | `source` (`display:<id>` ou `window:<id>`) | data URL JPEG, ou `""` | a miniatura do seletor |
| `list_cameras` | — | `[{id, …}]` | as câmeras, no Linux |
| `machine_cores` | — | número de núcleos | registrado; a interface não chama hoje |
| `start_broadcast` | `quality, fps, source, audio, muteCalls` | — | captura e encoder da tela |
| `stop_broadcast` | — | quadros enviados | para a tela; sem nenhuma origem subindo, solta o remetente e sorteia chave SRTP nova |
| `broadcast_stats` | — | `{active, …, encoder: "gpu" \| "cpu"}` | a linha de números da transmissão |
| `sfu_offer` | `source` (`screen`, `screenAudio`, `mic`, `camera`) | `{rtpParameters, srtpParameters}` | o corpo do `producePlain` |
| `use_sfu` | `address, serverKey` | — | aponta o remetente para a porta do `producePlain`; repetir o mesmo endereço não faz nada |
| `renew_sfu_key` | — | — | chave SRTP nova para republicar depois de o SFU reiniciar |
| `start_voice` / `stop_voice` / `set_voice_muted` | — / — / `muted` | — | o mic pelo Rust (Linux) |
| `start_camera` / `stop_camera` | `device` / — | — | a câmera pelo Rust (Linux) |
| `watch_key` | — | chave SRTP em base64 | a chave de recepção do `consumePlain` |
| `watch_native` | `producerId, kind, address, serverKey, payloadType, ssrc` | porta do MJPEG em 127.0.0.1 (0 no áudio) | assistir por RTP puro onde a janela não tem WebRTC (Linux) |
| `stop_watch` | `producerId`, ou `null` para tudo | — | só o `null` fecha o socket de recepção: o `comedia` do SFU aprendeu aquele endereço |
| `watch_mute` | `producerId, muted` | — | o Rust para de repassar o áudio da tela |
| `watch_stats` | — | pacotes recebidos | registrado; a interface não chama hoje |
| `google_login` | `server` (só http/https) | token do Sanctum | login pelo navegador do sistema, de volta por uma porta local |
| `open_url` | `url` (só http/https) | — | baixar o clipe pelo navegador do sistema |

## Rodar tudo local (para testar antes de subir)

Três processos e o app, todos na mesma máquina ou na mesma rede:

```bash
# 1. Laravel (API, site, painel) + Reverb (chat e presença)
cd web && composer dev            # serve em :8000, fila, logs, vite
cd web && php artisan reverb:start   # :8080

# 2. SFU (mídia). O segredo tem de ser o mesmo SFU_SECRET do web/.env
cd sfu && pnpm run build && SFU_SECRET=<o mesmo do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js

# 3. App apontando para o Laravel local (o SFU e o Reverb vêm do GET /api/config)
cd native/apps/desktop && VITE_SERVER=http://127.0.0.1:8000 npm run dev:app
```

Duas máquinas na mesma rede: troque `127.0.0.1` pelo IP da máquina que roda os
servidores em `APP_URL`, `SFU_PUBLIC_URL`, `REVERB_HOST` (`web/.env`), suba o SFU com
`SFU_HOST=0.0.0.0 SFU_ANNOUNCED_ADDRESS=<IP>`, e o Laravel com
`php artisan serve --host=0.0.0.0`. No WSL2 a rede só enxerga o UDP do SFU com
`networkingMode=mirrored` no `.wslconfig`.

Windows: o instalador sai de `C:\Users\edsu\unkvoid-build` como descrito em
[ESTADO.md](ESTADO.md); para apontar para o Laravel local sem rebuildar, grave
`localStorage.server = 'http://<IP>:8000'` no console do app.
