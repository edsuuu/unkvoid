# Servidores, canais, voz e chat — o contrato entre as três peças

> É a referência que Laravel (`web/`), SFU (`sfu/`) e app (`native/`) implementam: tudo o que
> atravessa a rede. Mudou aqui, muda nos três, na mesma tarefa. Chamava-se `SERVIDORES.md` até
> 19/09/2026.

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
| `ping` | `{}` | `{}` — vale sem ter entrado em sala |

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

Cada pessoa em `peers` (resposta do `join`) vem como
`{ peerId, userId, name, reconnecting, producers: [{ producerId, kind, source, paused }] }`.
Na entrada nova, quem está na carência de reconexão fica de fora da lista. Na **retomada**
(`resumed: true`) essa pessoa vem junto, com `reconnecting: true`: o app compara a lista com
a que já tinha e reproduz o que perdeu na queda (`peerJoined`, `peerLeft`, `newProducer`,
`producerClosed`, `producerPaused`/`Resumed`, `peerConnectionLost`/`Reconnected`). Sem quem
caiu junto, ele não saberia dizer se a pessoa saiu ou só está voltando.

**Sinalização viva é medida dos dois lados.** O servidor manda ping de WebSocket a cada 15 s
e derruba quem não responde. O navegador responde sozinho, mas não conta ao app que o socket
morreu: em 20/09/2026 o TCP da sinalização sumiu com a mídia (UDP) inteira, a carência
expirou, a mídia foi destruída e o app só soube 3,5 min depois. Por isso o app manda `ping` a
cada 5 s; sem resposta em 10 s ele larga o socket e reconecta com a `resumeKey`. Qualquer
resposta, até erro, prova que o socket vive — só o silêncio derruba.

**A retomada vale também com a sessão ainda de pé.** O app costuma perceber a queda antes do
heartbeat do servidor. `join` com `resume: true` e a `resumeKey` de uma sessão que ainda não
ficou órfã troca o socket dela (`resumed: true`, mesma `peerId`, mídia intacta), e o socket
velho é fechado sem abrir carência nem avisar a sala. Antes isso virava entrada nova, que
derrubava a antiga e a mídia com ela. A regra da conta continua: token de outro `sub` não
retoma.

**Na retomada vale o `can` do token novo.** O app pede token antes de cada `join`, inclusive
na reconexão, e quem decide permissão é o Laravel: se nos 30 s de carência a pessoa perdeu
`stream` ou foi mutada, a sessão retomada obedece o token que chegou agora, e não o da
entrada. Producer de origem que o `can` novo não cobre é fechado, e a sala recebe o
`producerClosed` de sempre. Se o que fechou foi a tela (`screen`, `screenAudio`), o dono
recebe `producerDead { producerId, kind, source, reason: 'revoked' }`; o `producerDead` do
relógio de 30 s sem pacote continua vindo **sem** `reason`. Microfone e câmera revogados
fecham calados para o dono: o app se acerta pelo `can` que volta no `join` retomado — e o app
antigo derrubava a tela com qualquer `producerDead`, fosse de que origem fosse. O token tem de
ser da **mesma conta** da sessão caída: com `sub` diferente o `join` não retoma, vira entrada
nova (senão uma conta herdaria o `can` de outra pela `resumeKey`).

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

Quem está logado entra na sala por código **com token**: `POST /api/rooms/{code}/token`
(`auth:sanctum`, código de 3 a 32 caracteres e nunca 26) devolve o mesmo
`{ token, url, expires_in }` da voz, com `room` = o código, `sub` = a conta e
`can: ["speak", "stream", "video"]`. É assim que a regra de uma sessão por conta vale
também ali: abrir a mesma chamada em outro dispositivo derruba o anterior. O `join` sem
token fica só para quem não tem conta.

O `join` sem token (sala anônima) recusa sala de 26 caracteres: é o formato do ULID de
canal, e sem isso qualquer um entraria num canal de voz sem passar pelo Laravel.

HTTP assinado (cabeçalhos `x-unkvoid-timestamp` e `x-unkvoid-signature`, assinatura
sobre `ts\nMÉTODO\ncaminho\ncorpo`, janela de 300 s — como o `kick` de hoje):

| rota | corpo | resposta |
|---|---|---|
| `POST /rooms/:code/kick` (já existe) | `{ "userId": "user:12" }` | `{ kicked: n }` |
| `POST /broadcast` | `{ channel, event, data }` — o tempo real do Laravel | `{ delivered: n }` (quantos sockets inscritos receberam) |
| `POST /rooms/:code/mute` | `{ "userId": "user:12", "muted": true }` — pausa/retoma o producer `mic` daquela conta | `{ muted: n }` |
| `GET /presence` | corpo vazio | `{ rooms: { "<room>": [ { sub, name, sources: ["mic","screen"] } ] } }` |

`consumePlain` devolve também `ssrc` do consumer: o receptor nativo do Linux separa os
producers de uma mesma porta por SSRC, sem adivinhar pelo primeiro pacote.

E devolve `rtx: { ssrc, payloadType } | null` — o fluxo de retransmissão do consumer
(RFC 4588), quando o codec tem um. É por ele que o receptor nativo (`media::PlainReceiver`,
usado pelo macOS, pelo Windows e pelo Linux) recupera pacote perdido: manda um NACK
(RTCP PT 205, FMT 1) com os números que faltaram, o SFU reenvia pelo `rtx.ssrc` com o
número original nos dois primeiros bytes do payload, e o quadro sai inteiro. Quando a
espera passa de 250 ms o buraco é largado e vai um PLI (PT 206, FMT 1) pedindo keyframe.
Os dois RTCP saem cifrados (SRTCP) pelo mesmo socket e com a mesma chave do `consumePlain`.
Sem `rtx`, o receptor ainda reordena e pede keyframe; só não recebe o reenvio.

Webhook do SFU para o Laravel, **fora do caminho do `join`**, fire-and-forget, para conta
(`user:`) e visitante da sala por código (`guest:<installId>`, `room` com o código de 3 a
32 caracteres). Em sala por código (qualquer `room` que não tenha 26 caracteres) o aviso só vira linha
em `guest_accesses` (nome, sala, IP, entrada e saída; `install_id` é o id da instalação do
visitante, ou `user:<id>` de quem entrou logado), sem tela que a mostre por enquanto: não há canal nem conta a
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

Banda no RTP puro: o app baixa a taxa do encoder quando o SFU pede muito pacote de volta (o
`nack` que o vídeo já declara) e sobe de novo quando a perda some, entre um piso e o teto da
qualidade escolhida. Nada muda no protocolo. O `goog-remb` declarado no codec é letra morta: o
mediasoup só estima banda quando o producer também traz a extensão `abs-send-time`, e estimar
por atraso sem um pacer no remetente acusaria congestionamento a cada quadro-chave.

## Laravel — API (`auth:sanctum`, JSON)

Tudo devolve `Resource`. Erro de permissão é 403 com `{ "message": "…" }`; validação é
422 no formato padrão do Laravel; hierarquia recusada é 403.

`GET /api/config` (público):
```json
{ "sfu": "ws://127.0.0.1:3000/sfu" }
```

Conta:

| rota | corpo | resposta |
|---|---|---|
| `POST /api/auth/register` (público) | `{ email, password, device, refresh? }` | `{ token, refresh_token, expires_at, user }`. O apelido nasce de `User::freeNickname` sobre o e-mail, com `nickname_confirmed: false` |
| `POST /api/auth/login` (público) | `{ email, password, device, refresh? }` | `{ token, refresh_token, expires_at, user }`. Senha errada é 401 com `"E-mail ou senha não conferem."` |
| `POST /api/auth/refresh` (público) | `{ refresh_token }` | o par novo, no mesmo formato; o token de renovação usado morre na hora. Vencido, usado ou de outra coisa: 401 `"Sua sessão terminou. Entre de novo."` |
| `POST /api/auth/logout` | `{ refresh_token? }` | 204; derruba o token de acesso da requisição e, se vier, o de renovação da mesma conta |
| `GET /api/me` | — | `{ id, name, email, avatar_url, avatar_uploaded, admin, nickname_confirmed }` |
| `PATCH /api/me` | `{ name }` (3 a 32 caracteres, `[A-Za-z0-9._]`, único; pode repetir o atual) | o mesmo `user`, agora com `nickname_confirmed: true`. Só enquanto `nickname_confirmed` for `false`: depois é 403 |
| `POST /api/me/avatar` | `multipart`, campo `avatar` (jpeg/png/webp, ≤ 2 MB) | o mesmo `user`, com a foto nova; guarda no bucket privado e apaga a foto anterior |
| `DELETE /api/me/avatar` | — | o mesmo `user` (200, não 204): tirar a foto enviada faz voltar a valer a do Google, e o app precisa do link novo |

**A renovação.** Quem manda `refresh: true` (o núcleo nativo manda sempre) recebe o par: o
token de acesso vence em **24 h** (`expires_at`) e o de renovação em **60 dias**, e esse só
serve para trocar o par em `/api/auth/refresh` — como `Bearer` ele não abre rota nenhuma. Quem
não manda (o app do Tauri até a 0.0.40) recebe o token de sempre, que não vence, e
`refresh_token`/`expires_at` nulos. O login do Google faz o mesmo com `refresh=1` em
`/oauth2/app`: o `refresh_token` volta na porta local ou no `unkvoid://` ao lado do `token`.

No núcleo, um 401 com sessão aberta renova o par uma vez (uma renovação por vez) e repete o
pedido; se a renovação falha, a sessão acaba: os tokens saem do disco e a interface volta à
entrada com "Sua sessão terminou. Entre de novo." Sair da conta nunca espera a rede.

`avatar_url` é a foto que a pessoa enviou, pré-assinada e vencendo em 2 h; sem foto enviada é o link permanente do Google, e sem nenhuma das duas vem `null`.
`avatar_uploaded` diz qual das duas é, e é o que decide se o app mostra "remover a foto". Toda
imagem enviada vira uma linha em `files` (caminho no bucket, quem enviou, tipo e tamanho) e a
conta aponta para ela por `avatar_id`.

`nickname_confirmed` é `users.nickname_confirmed_at` não nulo. Nasce nulo no cadastro pelo app
e na conta nova pelo Google (os dois ganham um apelido automático); nasce preenchido no
cadastro pelo site, onde a pessoa digita o apelido. As contas de antes da coluna vieram
preenchidas.

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
| `POST /api/servers/{server}/icon` | `multipart`, campo `icon` (jpeg/png/webp, ≤ 2 MB) | `ServerResource` (`MANAGE_SERVER`); guarda no mesmo bucket privado e apaga o arquivo antigo |
| `DELETE /api/servers/{server}/icon` | — | 204 (`MANAGE_SERVER`); volta ao ícone padrão |
| `GET /api/servers/{server}/audits` | — | as 50 entradas mais recentes do histórico do servidor (`VIEW_AUDIT_LOG`) |

`icon_url` é pré-assinada e vence em 2 h, como a foto de perfil; sem ícone vem `null`.

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
| `GET /api/channels/{channel}/messages?before={id}` | — | 50 mais recentes antes de `before`, ordem crescente: `[ { id, channel_id, user: {id,name,avatar_url}, type, body, files, reply_to, edited_at, created_at } ]` |
| `POST /api/channels/{channel}/messages` | JSON `{ body }` (1–2000), `reply_to_id` opcional; ou `multipart` com `images[]` (1 a 3 arquivos, jpeg/png/webp/gif, ≤ 2 MB cada) e aí o `body` é opcional (0–2000) | `MessageResource` (`SEND_MESSAGES`). O `reply_to_id` tem de ser de mensagem **do mesmo canal**, senão 422: aceitar id de fora vazaria texto de canal que a pessoa talvez nem enxergue |
| `PATCH /api/messages/{message}` | `{ body }` | só o autor. As imagens não mudam; o `body` só pode ficar vazio em mensagem que tem imagem |
| `DELETE /api/messages/{message}` | — | autor ou `MANAGE_MESSAGES`. Apaga também as imagens do bucket e as linhas de `files`: quem apagou uma foto mandada por engano não pode deixá-la no ar |

`files` é `[ { id, url, mime_type, size } ]`, vazio quando a mensagem não tem imagem, e vai
igual no `MessageSent` e no `MessageUpdated`. `url` é pré-assinada e vence em 2 h, como a foto
de perfil; ao reconectar o app busca as mensagens de novo e ganha links novos. Cada imagem é
uma linha em `files` ligada pela pivô `message_files`. O teto de 3 × 2 MB não é gosto: o PHP
da VPS aceita 2 MB por arquivo e 8 MB por pedido (`upload_max_filesize`, `post_max_size`), e o
app reduz a imagem antes de enviar para caber. Mensagem direta ainda não leva imagem: não há
pivô para ela, e criar é migration.

**Canal de voz também tem chat.** As rotas e os eventos acima valem para `type: voice` com as
mesmas permissões (`VIEW_CHANNEL` para ler, `SEND_MESSAGES` para escrever); o app mostra esse
chat ao lado do palco de quem está naquela voz.

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

Tempo real (as duas rotas que fazem o chat e a presença andarem sem o Reverb):

| rota | corpo | resposta |
|---|---|---|
| `POST /api/sfu/authorize` (assinada, sem Sanctum) | `{ sub: "user:12", channel: "channel.01j7…", at }` | `{ allowed, name }` **sem o `data`** — é o único JSON da API que sai cru, porque quem lê é o SFU |
| `POST /api/sfu/session` (`auth:sanctum`) | — | `{ token, url, expires_in: 60 }` — o token do `identify`, no mesmo formato do de voz, mas com as claims `{ sub, name, exp }` e **sem `room` nem `can`** |

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

## Tempo real (pelo SFU)

O mesmo WebSocket da mídia (`{SFU}` do `GET /api/config`) carrega o chat e a presença: não
há mais Reverb, nem `/broadcasting/auth`, nem `laravel-echo`. Depois de conectar, o app:

1. `POST /api/sfu/session` no Laravel e manda a ação `identify` com o `{ token }` que voltou —
   o socket passa a ter dono (`sub`, `name`), e o token vale 60 s (com 30 s de folga);
2. `subscribe` com `{ channel }` para cada canal que quer ouvir, e `unsubscribe` para largar.
   A cada `subscribe` o SFU pergunta ao Laravel (`POST /api/sfu/authorize`) se aquela conta
   pode ouvir aquele canal; recusa vira erro de permissão e o socket não entra.

O `subscribe` devolve a presença do canal, e o SFU emite `presence.joining { id, name }` e
`presence.leaving { id }` para os outros inscritos na primeira e na última conexão de cada
pessoa naquele canal.

O que o Laravel publica sai por `POST /broadcast` assinado, um pedido por canal. Falha do
SFU não desfaz nada: a escrita já está no banco, a resposta HTTP sai normal e o erro fica no
log do canal `sfu`. Como nada é reentregue depois de uma queda, ao reconectar o app busca de
novo pela API os servidores, a árvore aberta, amigos, a lista de conversas, e as 50 mensagens
mais recentes do canal e da conversa abertos, emendando com o que já estava na tela.

| canal | quem entra | eventos |
|---|---|---|
| `channel.{ulid}` | `VIEW_CHANNEL` | texto: `MessageSent { message }`, `MessageUpdated { message }`, `MessageDeleted { id, channel_id }` · voz: `VoiceStateUpdated { channel_id, user_id, name, event: joined\|left }` (no canal da própria voz, para canal oculto não vazar quem está nele; o app assina o canal de cada voz que enxerga) |
| `server.{id}` | membro | `ServerUpdated { server_id }` (qualquer mudança de estrutura: o app refaz o `GET`) |
| `user.{id}` | o próprio | `FriendshipUpdated { friendship, removed }` (`FriendResource`, nos canais dos **dois** lados) · `MemberRemoved { server_id, reason: kicked\|banned }` · `DirectMessageCreated { message, recipient }`, `DirectMessageUpdated { message, recipient }`, `DirectMessageDeleted { id }` (nos canais dos **dois** lados da conversa; `message` é o `DirectMessageResource` sem o `mine`) |

O nome do canal perdeu os prefixos `private-` e `presence-` do Pusher: é `channel.`, `server.`
e `user.`, o mesmo nome dos dois lados.

Nos apps nativos o cliente é o `core_app::realtime::Realtime`: ele se apresenta, reassina
sozinho cada canal quando o socket volta (e avisa `realtime.back`), e emite `presence.here`
com quem já estava num canal de presença ao assinar. O que cada evento significa — reler o
canal, a conversa, a árvore, o toque de mensagem, o aviso — é o `realtime::read`, e as
interfaces só executam.

### Expulsar e banir cortam a pessoa de tudo

Vale a partir do momento em que acontece; quem já tinha saído antes não é reprocessado.

- **Voz:** o Laravel chama o `kick` do SFU procurando a pessoa na presença fresca (sem o cache
  de 3 s). O token de voz de antes do kick vale 60 s: o webhook `joined` de quem já não é
  membro chama o `kick` na hora, sem abrir acesso nem emitir `VoiceStateUpdated`.
- **Tempo real:** `MemberRemoved` no `user.{id}` faz o app sair dos canais do servidor, e
  assinar de novo é recusado pelo `POST /api/sfu/authorize`. **Limite:** quem já estava
  inscrito continua inscrito — o SFU não refaz a pergunta sozinho. Um cliente modificado que
  ignore o `MemberRemoved` recebe os eventos dos canais que já assinava até reconectar.

## App — o que aparece

- Entrada: a tela de código continua; ao lado, "Entrar" (e-mail/senha ou Google pelo
  `/oauth2/app?state=`, de volta pelo `unkvoid://`) e "Criar conta". Token do Sanctum em `localStorage`
  (`unkvoid:token`). Com token válido (`GET /api/me`), abre o modo servidor. Criar conta pelo
  app é só e-mail e senha.
- Com `nickname_confirmed: false`, um modal que não fecha pede o apelido (já preenchido com o
  automático) a cada abertura do app, até o `PATCH /api/me` dar certo. Dá para sair da conta
  por ele.
- Nos formulários de entrar e criar conta, o campo recusado fica com a borda vermelha e a
  mensagem embaixo dele; o que não é de campo (credencial errada, 429) fica na linha geral.
- Logado e sem servidor aberto, o centro mostra **Criar sala** e **Últimas salas**. Criar
  sala é `POST /api/servers { name }` (servidor com `#geral` e `Geral` de voz): abre o
  servidor, **não** entra na voz, e mostra o código de convite para mandar. Últimas salas
  é o `GET /api/servers`: abrir uma mostra quem está em cada voz, e entrar é um clique. A
  sala por código sem login continua como está.
- Entrar numa voz liga o microfone **aberto**; quem prefere entrar calado marca "Silenciar ao
  entrar" nas configurações. (Até a 0.0.39 o padrão era mutado.)
- O clique no canal de voz vale na hora: a pessoa já aparece na lista do canal e a barra de voz
  mostra "Conectando…" enquanto o token, o SFU e o microfone acontecem por trás. Se a entrada
  falhar, ela sai da lista. O ícone de mudo da própria pessoa na lista do canal segue o estado
  local (mutar, ensurdecer, mudo do servidor), sem esperar ninguém.
- Modo servidor: trilho de servidores | canais (texto e voz, quem está em cada voz) |
  centro (chat ou palco) | membros com cargos. Barra de voz embaixo: mutar, ensurdecer,
  câmera, **compartilhar tela (só aqui)**, sair.
- Windows/macOS: mic e câmera pelo `getUserMedia` + `sendTransport.produce`. Linux:
  pelo Rust (`pulsesrc`/`v4l2src` → RTP puro), como a tela.
- Áudio de `screenAudio` chega **mudo**. `mic` toca direto. `camera` vira cartão pequeno.
- Chat: até 3 imagens por mensagem, por botão, colando ou arrastando; o app reduz cada uma para
  caber em 2 MB antes de enviar. Quem está numa voz tem o chat daquele canal ao lado do palco.
- Cada pessoa da voz tem volume e mudo locais (guardados por conta), e as configurações têm
  "Saída de áudio" onde o motor da janela tem `setSinkId` (WebView2). No Linux a voz dos outros
  toca pelo Rust, então esses dois controles não aparecem lá.
- Variáveis de ambiente do app, para calibrar e diagnosticar: `UNKVOID_ENCODER=cpu` (pula o
  encoder da placa), `UNKVOID_ABR=off` (taxa fixa, sem acompanhar a perda),
  `UNKVOID_CAPTURE=x11|portal` (força a captura do Linux).

## App — a ABI do núcleo (interfaces nativas)

O Swift fala com o `native/shared/core` por uma ABI C (`shared/core/src/ffi.rs`); o header
está em `native/apps/macos/Sources/UnkvoidCore/include/unkvoid_core.h`. Os apps do Linux
(GTK) e do Windows (Slint) não passam por ela: são Rust e usam o `core` como crate.

Tudo o que muda dentro do núcleo vive atrás de cadeado: a interface pode ler eventos e
mídia em outras threads **enquanto** uma ação está em voo.

| Função | Devolve |
|---|---|
| `unkvoid_core_new()` | o ponteiro do núcleo, ou nulo |
| `unkvoid_connect(h, url)` | `true` se o socket do tempo real abriu. É o socket do chat e da presença; ele cai e volta sozinho (`realtime.lost`, `realtime.back`). A sala abre outro |
| `unkvoid_call(h, ação, json)` | a resposta do SFU nesse socket (`subscribe`, `unsubscribe`, `ping`). **Bloqueia**, no máximo 10 s |
| `unkvoid_app(h, ação, json)` | as decisões do app. **Bloqueia** no que fala com o servidor |
| `unkvoid_next_event(h)` | o próximo aviso, ou nulo. Não bloqueia |
| `unkvoid_next_media(h, *tamanho)` | o próximo quadro ou bloco de som do que se assiste; espera até 100 ms e devolve nulo. Para **uma** thread só da interface |
| `unkvoid_speak(h, *amostras, n)` | o microfone que a interface capturou: PCM `f32` estéreo intercalado a 48 kHz |
| `unkvoid_show(h, IOSurfaceRef, ns)` | macOS: um quadro da câmera, no buffer de GPU, **já retido** — quem solta é o núcleo |
| `unkvoid_string_free(texto)`, `unkvoid_bytes_free(bloco, tamanho)` | devolvem o que o núcleo alocou — uma vez só |

O bloco de `unkvoid_next_media`, little-endian:
`[tipo u8: 0 vídeo, 1 som][keyframe u8][tamanho do id u16][timestamp u32][id do producer][conteúdo]`.
Vídeo é um quadro H.264 inteiro em Annex-B (o `timestamp` é o do RTP, 90 kHz); som é PCM
`f32` estéreo intercalado a 48 kHz, já decodificado do Opus. Quem decodifica o vídeo é a
interface, com o que o sistema tem (`AVSampleBufferDisplayLayer` no macOS).

As ações de `unkvoid_app`:

| Ação | Entra | Sai |
|---|---|---|
| `state` | — | `{screen, name, room, signedIn}` |
| `createRoom`, `joinRoom` | `{name, code}` | `{ok, room}`, `{refused}` ou `{failed}`. **Entra de verdade no SFU**: se ele recusar, a tela não muda |
| `joinVoice` | `{channel}` | `{ok, room}`: a mesma sala, com o token de voz de 60 s |
| `leaveRoom`, `recentRooms` | — | `{ok}` / `{rooms}` |
| `room` | — | `{peers, tiles, mine}`: a sala de agora, sem esperar aviso |
| `displays` | — | `{portal, displays: [{id, width, height}], windows: [{id, title, application}]}` |
| `share` | `{source, quality, fps, audio, muteCalls}` | `{ok}`. `source` é `display:<id>` ou `window:<id>`; `quality` é `720`, `1080`, `1440` ou `2160` |
| `stopSharing` | — | `{ok}` |
| `openMicrophone`, `closeMicrophone` | — | `{ok}`: abre o producer do microfone; o som entra por `unkvoid_speak` |
| `muteMicrophone` | `{muted}` | `{ok}`: manda silêncio e pausa o producer |
| `openCamera`, `closeCamera` | `{width, height, fps}` / — | `{ok}` (macOS): o quadro entra por `unkvoid_show` |
| `deafen` | `{deafened}` | `{ok}`: cala o som que chega, sem pausar o vídeo |
| `muteWatched` | `{producerId, muted}` | `{ok}`: o som de uma tela, que chega mudo por regra |
| `closeWatched`, `pauseWatched` | `{producerId}` / `{producerId, paused}` | `{ok}`: fecha ou pausa uma transmissão só para esta pessoa |
| `watch` | `{producerId?}` | `{ok}`: reabre o que foi fechado; sem `producerId`, tudo |
| `selfView` | `{wanted}` | `{ok}`: "ver o que a sala vê" — assistir à própria tela |
| `changeQuality` | `{quality, fps}` | `{ok}`: troca resolução e quadros com a transmissão no ar |
| `preview` | `{source}` | `{jpeg}`: a miniatura de uma tela ou janela, em base64 (vazia sem permissão) |
| `inputMode` | `{mode, sensitivity}` | `{ok}`: `voice` (abre acima de `sensitivity`, 0–100), `ptt` ou `open` |
| `talk` | `{talking}` | `{ok}`: a tecla de apertar para falar desceu ou subiu |
| `useServer` | `{url}` | `{ok}` |
| `config` | — | `{sfu}`: o endereço do SFU que o Laravel anuncia |
| `login`, `register` | `{email, password, device}` | `{ok, user}`. Como o `signOut`, já deixa o `state` na tela certa: quem decide onde se cai é o núcleo |
| `me` | — | `{ok, user}`; com o token vencido, `{failed: signedOut}` e o token sai do disco |
| `signOut` | — | `{ok}` |
| `identify` | — | `{ok}`: apresenta a conta ao tempo real com o token de `/api/sfu/session` |
| `servers` | — | `{servers}` |
| `server` | `{id}` | `{server}` |
| `messages` | `{channel}` | `{messages}` |
| `sendMessage` | `{channel, body}` | `{ok, message}` |
| `editMessage` | `{id, body}` | `{ok, message}` |
| `deleteMessage` | `{id}` | `{ok}` |
| `api` | `{name, params, body}` | `{ok, data}`: uma rota do Laravel pelo **nome** (`shared/core/src/routes.rs`): `createServer`, `kickMember`, `putOverwrite`, `friends`, `sendDirect`… `params` preenche o caminho (`{server}`, `{user}`) e pode levar `query` |
| `upload` | `{name, params, field, files, fields}` | `{ok, data}`: o mesmo, em `multipart` — foto, ícone do servidor, imagens de uma mensagem. `files` são caminhos no disco |
| `preference`, `setPreference` | `{key}` / `{key, value}` | `{value}` / `{ok}`: o que se guarda em disco, com as chaves do app de hoje (`unkvoid:voice`…). O token não sai por aqui |
| `keys`, `keyName` | `{accelerator}` / `{code}` | macOS: um atalho (`CmdOrCtrl+Shift+KeyM`) nos códigos do sistema, e o caminho de volta |
| `googleStart`, `googleWait` | — | `{ok, url}` e depois `{ok, user}`: login com Google por uma porta local (`google.rs`); a segunda espera até 5 min |
| `logs` | `{lines}` | `{path, lines}`: o fim do registro do núcleo |
| `update` | — | `{version, url}` se há versão mais nova publicada para esta plataforma, senão `{upToDate}` |

A ação `server` devolve também `abilities` — o que esta pessoa pode fazer ali, já calculado
(`permissions.rs`): `{can: [nomes], owner, members: {<user_id>: {nickname, mute, deafen,
disconnect, kick, ban, roles}}, roles: [{id, editable, assignable, up, down}]}`. A interface
só esconde botão com isso; quem autoriza é o Laravel, em toda chamada.

`screen` é `entry`, `hub`, `room`, `offline` ou `updating`.

Os avisos de `unkvoid_next_event` são `{event, channel, data}`. Os do tempo real vêm como o
SFU os manda (`MessageSent`, `presence.joining`…), com o `channel` de onde vieram. Os da
sala aberta não têm canal:

| Aviso | `data` |
|---|---|
| `room.peers` | `{peers: [{peerId, userId, name, producers, reconnecting, selfPeer}]}`, a lista inteira |
| `room.tiles` | `{tiles: [{producerId, peerId, label, camera, mine, paused, audio}], pending: […]}`: o que dá para desenhar, e o que está ao vivo e a pessoa fechou; `audio` é o producer do som daquela tela |
| `room.mine` | `{sharing, selfView, mic, micMuted, camera, canShare, canSpeak, canVideo}`: o que esta pessoa manda, e o `can` do servidor |
| `room.watchers` | `{producerId, watchers: [{peerId, name}]}`: quem está assistindo àquela tela |
| `room.session` | `{state: "lost" \| "rejoined" \| "gone" \| "replaced" \| "kicked"}`. Os dois últimos são o servidor tirando esta sessão de propósito: o núcleo **não** volta sozinho, e a interface tira a pessoa da sala |
| `room.failed` | `{what: "watch" \| "share" \| "mic"}` |
| `room.ping` | `{ms}`: a ida e volta da sinalização, a cada 5 s |
| `room.level` | `{level, percent}`: o nível do microfone — de 0 a 1, e na escala de 0 a 100 da sensibilidade —, uns dez por segundo |
| `room.chime` | `{chime: "joined" \| "left" \| "streamStarted" \| "streamStopped"}`: o toque de uma troca do elenco (`chimes.rs`); a interface só toca `Chime::samples()` |
| `room.notice` | `{text}`: o aviso do React junto com o toque — "fulano entrou", "saiu", "começou a transmitir" |
| `realtime.lost`, `realtime.back` | `{}`: na volta a interface se apresenta e se inscreve de novo |
| `session.ended` | `{}`: o par não renovou e a sessão acabou; a interface volta à entrada |

**Falha nunca atravessa com detalhe técnico.** Vem `{failed: "<motivo>"}` — `unreachable`,
`signedOut`, `notAllowed`, `gone`, `invalid`, `serverBroke`, `tooFast` — e a interface
escreve a frase em português. A exceção é validação: `{invalid: {field, message}}`, que o
Laravel já devolve em português e sobre o campo digitado. Caminho, endereço e código de
status ficam no log.

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
| `list_displays` | — | `[{id, width, height, portal}]` | as telas do seletor. No Linux em sessão Wayland vem **um** item, `{id: 1, width: 0, height: 0, portal: true}`: quem lista e escolhe é o seletor do próprio sistema |
| `list_windows` | — | `[{id, title, application}]` | os aplicativos do seletor (vazio no Wayland) |
| `source_preview` | `source` (`display:<id>` ou `window:<id>`) | data URL JPEG, ou `""` | a miniatura do seletor (`""` no Wayland) |
| `list_cameras` | — | `[{id, …}]` | as câmeras, no Linux |
| `machine_cores` | — | número de núcleos | registrado; a interface não chama hoje |
| `start_broadcast` | `quality, fps, source, audio, muteCalls` | — | captura e encoder da tela. No Wayland o `source` é ignorado: abre o seletor do sistema (monitor ou janela) e só resolve quando a pessoa escolhe; cancelar rejeita com `screen picker closed without choosing a source` |
| `change_broadcast_quality` | `quality, fps` | — | troca resolução e fps no meio da transmissão, sem fechar os producers (no Wayland reaproveita a sessão do portal: o seletor não abre de novo) |
| `stop_broadcast` | — | quadros enviados | para a tela; sem nenhuma origem subindo, solta o remetente e sorteia chave SRTP nova |
| `broadcast_stats` | — | `{active, …, encoder: "gpu" \| "cpu", targetBitrate, lossPermille}` | a linha de números da transmissão. `targetBitrate` (bits por segundo) é a taxa que o governador de perda pediu ao encoder; `lossPermille` (0 a 1000) é a perda da última janela de ~1 s com tráfego — tela parada não atualiza, o valor anterior fica |
| `sfu_offer` | `source` (`screen`, `screenAudio`, `mic`, `camera`) | `{rtpParameters, srtpParameters}` | o corpo do `producePlain` |
| `use_sfu` | `address, serverKey` | — | aponta o remetente para a porta do `producePlain`; repetir o mesmo endereço não faz nada |
| `renew_sfu_key` | — | — | chave SRTP nova para republicar depois de o SFU reiniciar |
| `set_shortcuts` | `bindings: [{action, accelerator}]` | `{registered, failed}` | atalhos do sistema (mutar, ensurdecer, falar apertando); cada tecla disparada chega no evento `shortcut` com `{action, pressed}`. No Windows **nenhuma** ação passa pelo registro de atalho do sistema, que engole a tecla (o jogo deixa de recebê-la) e não enxerga o mouse: o Rust consulta o estado das teclas a cada 20 ms e só avisa a interface, então a tecla chega ao jogo e ao app ao mesmo tempo. Vale para `mute`, `deafen` e `talk`; modificador a mais não impede (quem corre com Shift no jogo ainda muta), e `Mouse3`, `Mouse4` e `Mouse5` valem como tecla de falar. No macOS e no Linux continua o registro do sistema. O formato do `accelerator` é o mesmo: modificadores + código (`Control+KeyV`, `KeyV`, `Mouse4`) |
| `start_voice` / `stop_voice` / `set_voice_muted` | — / — / `muted` | — | o mic pelo Rust (Linux). De `start_voice` a `stop_voice` sai o evento `voice:level` com `{ level }` (RMS linear de 0 a 1, o maior de cada janela de 100 ms): é o que a detecção de voz da interface mede, já que ali o áudio não passa pela janela. Sai **mesmo mutado** — é ele que reabre o portão. Mutado, o Rust manda silêncio em Opus em vez de nenhum pacote: sem pacote o relógio de 30 s do SFU mataria o producer |
| `start_camera` / `stop_camera` | `device` / — | — | a câmera pelo Rust (Linux) |
| `watch_key` | — | chave SRTP em base64 | a chave de recepção do `consumePlain` |
| `watch_native` | `consumer: { producerId, kind, address, serverKey, payloadType, ssrc, rtx }` | porta do MJPEG em 127.0.0.1 (0 no áudio) | assistir por RTP puro onde a janela não tem WebRTC (Linux); `rtx` é o do `consumePlain`, para o reenvio de pacote perdido |
| `stop_watch` | `producerId`, ou `null` para tudo | — | só o `null` fecha o socket de recepção: o `comedia` do SFU aprendeu aquele endereço |
| `watch_mute` | `producerId, muted` | — | o Rust para de repassar o áudio da tela |
| `watch_stats` | — | pacotes recebidos | registrado; a interface não chama hoje |
| `google_login` | `server` (só http/https) | token do Sanctum | login pelo navegador do sistema, de volta pelo `unkvoid://login?token=&state=` |

## Rodar tudo local (para testar antes de subir)

Três processos e o app, todos na mesma máquina ou na mesma rede:

```bash
# 1. Laravel (API e site). O chat e a presença saem pelo SFU, não há mais Reverb
cd web && composer dev            # serve em :8000, fila, logs, vite

# 2. SFU (mídia). O segredo tem de ser o mesmo SFU_SECRET do web/.env
cd sfu && pnpm run build && SFU_SECRET=<o mesmo do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js

# 3. App apontando para o Laravel local (o SFU vem do GET /api/config)
cd native/apps/desktop && VITE_SERVER=http://127.0.0.1:8000 npm run dev:app
```

Duas máquinas na mesma rede: troque `127.0.0.1` pelo IP da máquina que roda os
servidores em `APP_URL` e `SFU_PUBLIC_URL` (`web/.env`), suba o SFU com
`SFU_HOST=0.0.0.0 SFU_ANNOUNCED_ADDRESS=<IP>`, e o Laravel com
`php artisan serve --host=0.0.0.0`. No WSL2 a rede só enxerga o UDP do SFU com
`networkingMode=mirrored` no `.wslconfig`.

Windows: o instalador sai de `C:\Users\edsu\unkvoid-build` como descrito em
[BUILD-WINDOWS.md](BUILD-WINDOWS.md); para apontar para o Laravel local sem rebuildar, grave
`localStorage.server = 'http://<IP>:8000'` no console do app.
