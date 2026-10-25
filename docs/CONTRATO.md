# Servidores, canais, voz e chat — o contrato entre as três peças

> É a referência que Laravel (`web/`), SFU (`sfu/`) e app (`native/`) implementam: tudo o que
> atravessa a rede. Mudou aqui, muda nos três, na mesma tarefa. Chamava-se `SERVIDORES.md` até
> 19/09/2026.

A sala anônima por código **continua como está**: quem não quer conta abre o app,
cria uma sala e manda o código. O que este documento descreve é o segundo modo, só
para quem está logado: servidores com conta, em que o Laravel autoriza cada canal de voz e
assina o token que o SFU confere.

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
MANAGE_CHANNELS = 1 << 3     CONNECT         = 1 << 11     MANAGE_EXPRESSIONS = 1 << 18 (sons do soundboard)
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
  "can": ["speak", "stream", "video"], "muted": true }
```

`muted` só vai quando a pessoa está mutada pelo servidor (`server_mute`): o SFU a faz entrar
já calada (`serverMuted`), **na entrada nova e na retomada** (um `/mute` perdido se acerta na
primeira oscilação), e o `can` continua levando `speak` pela permissão `SPEAK`. É o que deixa o
`/mute false` devolver a voz a quem entrou mutado, sem token novo nem outra entrada; tirar o
`speak` do token, como era até 09/10/2026, prendia a pessoa calada até sair e voltar. O webhook
`joined` também chama `/mute true` para quem está mutado: é por esse `serverMuted` que o app
fica sabendo. O `join` devolve `serverMuted: boolean`.

O `join` também devolve `e2eeSessionId`, aleatório por instância viva de Room. O core combina-o
com o código da sala para derivar um `group_id` MLS que não se repete quando uma sala vazia é
recriada. Em `producePlain`, o app E2EE envia `e2eeMemberId` (chave pública de assinatura da
sessão MLS); o SFU apenas ecoa o identificador público no roster e no `consumePlain`.

Mutado pelo servidor, o `produce`/`producePlain` do `mic` **não** é recusado: o producer nasce
pausado (a resposta traz `paused: true`), e o `/mute false` o retoma — assim o app liga o mic como
sempre, sem erro, e a voz volta sem clique. Só `resumeProducer` do mic continua sendo 403
enquanto durar.

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
| `pauseProducer` | `{ producerId }` | `{ status: 'paused' }` |
| `resumeProducer` | `{ producerId }` | `{ status: 'resumed' }` |
| `closeConsumer` | `{ consumerId }` | `{ status: 'closed' }` |
| `ping` | `{}` | `{}` — vale sem ter entrado em sala |
| `voiceState` | `{ muted, deafened }` (os dois booleanos, obrigatórios; só dentro de sala) | `{ muted, deafened }` |
| `e2eeJoin` | `{ version: 1, group, payload }` — pedido de entrada com o `KeyPackage` MLS opaco | `{ status: 'forwarded', delivered }` |
| `e2eeCommit` | `{ version: 1, group, epoch, payload }` — Commit MLS opaco | `{ accepted, epoch, currentEpoch, reason? }` |
| `e2eeWelcome` | `{ version: 1, group, epoch, targetPeerId, payload }` — Welcome MLS opaco | `{ status: 'forwarded' }` |

### Sinalização MLS/E2EE (fase 4b, política Require no cliente 0.2.0)

As três ações exigem que o socket já tenha entrado na `Room`. `group` e `payload` são base64url
sem padding; `group` tem até 128 bytes e cada `payload` até 64 KiB depois de decodificado. O SFU
nunca desserializa MLS, cifra, chave de mídia ou identidade de membro: repassa os bytes exatamente
como chegaram e acrescenta no **evento** `senderPeerId` e `senderUserId`, tirados da sessão
autenticada — o cliente não escolhe quem é o remetente.

`e2eeJoin` vira o evento `e2eeJoinRequested` para os outros peers da sala, com
`{ version, group, kind: 'key_package', senderPeerId, senderUserId, payload }`. É o pedido para
o comitador atual acrescentar a pessoa que acabou de mandar o KeyPackage. O app seguirá a regra
MLS de escolher o membro ativo de menor leaf index como comitador; o SFU não tenta deduzir esse
índice de uma carga que deve permanecer opaca.

`e2eeCommit` tem ordenação atômica **por `group` e `epoch`**: o SFU aceita somente o primeiro
bloco cuja `epoch` seja a atual, responde `{ accepted: true, epoch, currentEpoch: epoch + 1 }` e
emite `e2eeCommit` aos demais peers. Outro bloco da mesma época recebe
`{ accepted: false, reason: 'different_commit_already_accepted', ... }`; época menor recebe
`stale_epoch`, e maior `future_epoch`. Reenviar pelo mesmo peer o mesmo Commit recém-aceito (a
resposta pode ter se perdido) volta como aceito. O autor só chama `confirm_pending()` no núcleo
depois da resposta aceita; se receber recusa, chama `discard_pending()`, processa o vencedor e
refaz a mudança se ainda for necessária.

`e2eeWelcome` só é encaminhado como evento homônimo ao `targetPeerId` quando foi enviado pelo
mesmo peer do último Commit aceito e para sua época (`currentEpoch == epoch + 1`). Antes disso,
por outro autor ou para um alvo em reconexão, é 409. Assim o novo membro só recebe Welcome depois
do aceite do Commit, sem o SFU abrir a mensagem.

Para limitar abuso, cada peer pode enviar no máximo 20 blocos E2EE e 256 KiB de payload em uma
janela móvel de 10 s; ultrapassar qualquer teto retorna 429. A Room mantém no máximo quatro
grupos E2EE ativos. Para cada um, mantém **somente** `currentEpoch` e o SHA-256 do último Commit
aceito, com o `peerId` autor, para responder a um reenvio idempotente. Não guarda KeyPackage,
Commit ou Welcome. Isso basta para serializar mudanças; quem entra tarde sempre manda KeyPackage
novo e recebe Welcome novo, enquanto histórico opaco guardado no SFU não o ajudaria a recuperar
com segurança. A memória desaparece junto com a Room vazia.

No `join`, o SFU devolve `e2eeSessionId`, aleatório por instância viva de Room. O core deriva
`group_id = SHA-256("unkvoid-mls-voice-room-v1" || len(room) || room || len(e2eeSessionId) ||
e2eeSessionId)`, portanto salas recriadas não reutilizam grupo. O primeiro peer cria o grupo;
os demais enviam KeyPackage em `e2eeJoin`. O membro ativo de menor leaf index adiciona entradas e,
ao sair o comitador, o próximo membro ativo o substitui. O cliente só confirma o Commit depois de
`accepted: true`; apenas então envia Welcome ao `targetPeerId`.

Na sala por código vale o mesmo: o `join` sem token devolve `userId` (`guest:<installId>`) e
`e2eeSessionId` como o `join` da conta, e as três ações aceitam o visitante. Ali o
`senderUserId` é a instalação que o próprio app escolheu: cifra contra o SFU, mas não autentica
ninguém — quem tem o código entra no grupo, e dois apps com o mesmo `installId` aparecem com a
mesma identidade.

`producePlain` leva `e2eeMemberId`, a chave pública de assinatura MLS da sessão que possui aquele
producer. O SFU a devolve no elenco e nas respostas `consumePlain`, sem interpretá-la. O consumidor
confere esse valor contra a árvore MLS autenticada antes de derivar a chave `(época, MemberId,
origem)`; ausência ou membro inativo falha fechado. Origens são `mic`, `screenAudio`, `screen` e
`camera`. Cifrar ocorre após Opus/H.264 e antes do RTP; decifrar após remontar a unidade e antes
do decoder. Em troca de época, o receptor aceita a época anterior por **3 segundos**, para pacotes
RTP já emitidos e em trânsito, e nunca a usa para estado MLS.

O core emite `room.encryption` com `{ state: establishing|encrypted|failed, reason?, privacyCode,
members }`; o código é de 30 dígitos, e cada membro tem `accountId`, `memberId` público e
`leafIndex`. Nenhuma chave privada nem chave de mídia sai do processo. O `epoch` do envelope é
metadado de ordenação do relé, não prova criptográfica: época, árvore e remetente são aceitos
somente após validação MLS. Mensagens E2EE não entram em logs, webhooks, `/presence` ou Laravel.
Na voz do cliente 0.2.0, `Policy::Require`: sem grupo/MemberId não se abre producer nem se aceita
quadro em claro; não há downgrade/interoperabilidade de mídia com um cliente antigo que mande
claro. O SFU não exige essas ações nem altera o fluxo de entrada por código (inclusive sem conta):
clientes que não usam E2EE ainda podem entrar pelo protocolo existente, sem Laravel flag ou rota
nova.

`Source` passa a ser `screen | screenAudio | mic | camera`. `producePlain` aceita os
quatro. `newProducer`, `producerClosed`, `consume`, `consumePlain` e `describePeers`
já carregam `source`; nada muda no formato deles. Os eventos ganham
`producerPaused { peerId, producerId }` e `producerResumed { peerId, producerId }`
para a sala inteira menos o dono.

Plateia: o SFU manda para a sala inteira `watchers { producerId, watchers: [{ peerId, name }] }`
— quem está **olhando** aquela transmissão agora. Três
regras que o cliente precisa saber para não contar errado:

- **só `source: screen`**. Câmera e microfone ficam de fora: numa sala cheia é todo mundo
  consumindo todo mundo, e o evento viraria enxurrada;
- **nascer não é assistir**. O consumer nasce pausado, então a plateia só muda no
  `resumeConsumer` e no `pauseConsumer` — e em `closeConsumer`, na queda do transporte, na
  perda de sinalização (a pessoa sai da lista na hora) e na retomada dentro da carência (ela
  volta);
- quem está na carência de reconexão não conta como plateia.

A resposta do `join` traz `elapsedMs`: há quanto tempo a sala existe, desde a primeira pessoa
— a sala nasce com ela e some com a última. O relógio da barra da sala conta daí, igual para
todo mundo. É duração, e não hora, porque o relógio de cada máquina erra (o de um PC estava
18 s à frente do da VPS em 30/09/2026); quem recebe subtrai do próprio relógio. Sem o campo
(SFU antigo), o relógio conta da própria entrada.

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
os bits do canal) para decidir se liga o mic, a câmera e a tela. Mutado pelo servidor chega
**com** `speak` e com a marca `muted` do token: o SFU recusa `produce`/`resumeProducer` do mic
enquanto durar, manda `serverMuted { muted }` para a própria pessoa quando o Laravel chama
`/mute` (também na chegada de quem já estava mutado), e é por esse evento que o app sabe.

**Uma conta, uma sessão no SFU inteiro.** O `join` com token (`sub` que não começa com
`guest:`) derruba qualquer outra sessão daquela conta, na mesma sala ou em outra, e ela
recebe `replaced { reason }` e perde o socket com o código 4002. A sala vê o `peerLeft` e
o Laravel o `left` da sala antiga. O visitante (`guest:`) só é substituído pela
`resumeKey`: o `installId` é escolhido pelo próprio app e a sala inteira o recebe no
`peerJoined`, então valer como identidade deixaria qualquer um derrubar qualquer um. O
app ignora `replaced`, `kicked` e `closed` de um `SfuClient` que já não é o atual.

**Worker do mediasoup que morre leva só as salas dele.** O socket de cada pessoa dessas salas
fecha com o código **1012** (servidor reiniciando), sem evento antes; o worker renasce no mesmo
lugar e com as mesmas portas. Para o app é uma queda como outra qualquer: reconecta com a
`resumeKey`, recebe `resumed: false` (a sala acabou) e publica de novo. As outras salas não
percebem nada. Sem nenhum worker vivo, o `GET /health` responde **503** e o `join` também; o
corpo do `/health` traz `workersDown`, quantos estão caídos agora.

**Mover alguém de voz é o `kick` com destino.** O `POST /rooms/:channel/kick` com `to` manda ao
peer `moved { to, by }` (`to` é o ULID do canal de destino, `by` o nome de quem moveu, ou
`null`) em vez de `kicked`, e fecha o socket com o código **4003** (`moved`; 4001 é `kicked`,
4002 é `replaced`). A sala recebe só o `peerLeft` de sempre, sem `peerKicked`, e o webhook
`left` sai normal. O app do movido **não** trata `moved` como queda: para o `SfuClient` antes do
fechamento (como no `kicked`), sai da sala sem o toque de saída, pede o token de voz do destino
ao Laravel e entra sozinho — o SFU só avisa. O app antigo, que não conhece `moved`, reconecta
na origem com token novo: por isso o Laravel recusa o token da origem por 60 s depois de mover.

**O estado de voz de cada pessoa (mudo e surdo) é dela, e o SFU só repete.** O app manda
`voiceState { muted, deafened }` logo depois de **cada** `join` (entrada nova nasce
`false/false`; a retomada guarda o que tinha) e a cada troca. O SFU guarda no `Peer`, devolve nos
`peers` do `join` (`muted`, `deafened` por pessoa) e no `GET /presence`, e publica ele mesmo, no
tempo real, `VoiceMuteUpdated { channel_id, user_id, name, muted, deafened }` em
`channel.<room>` — só em sala de 26 caracteres (canal), só para `sub` de conta (`user:N`), e só
quando o estado mudou. O nome não é `VoiceStateUpdated` de propósito: o app antigo tira da
lista quem chega nesse evento com `event` diferente de `joined`. Mudo e surdo **do servidor**
(`server_mute`, `server_deaf`) continuam vindo do Laravel, na árvore.

Quem está logado entra na sala por código **com token**: `POST /api/rooms/{code}/token`
(`auth:sanctum`, código de 3 a 32 caracteres e nunca 26) devolve o mesmo
`{ token, url, expires_in }` da voz, com `room` = o código, `sub` = a conta e
`can: ["speak", "stream", "video"]`. É assim que a regra de uma sessão por conta vale
também ali: abrir a mesma chamada em outro dispositivo derruba o anterior. O `join` sem
token fica só para quem não tem conta.

O `join` sem token (sala anônima) recusa sala de 26 caracteres: é o formato do ULID de
canal, e sem isso qualquer um entraria num canal de voz sem passar pelo Laravel. Com token, o
`room` tem de ser o ULID minúsculo de um canal (`POST /api/channels/{channel}/voice/token`) ou
um código `[a-z0-9-]` de 3 a 32 caracteres (`POST /api/rooms/{code}/token`); outro formato é
422.

HTTP assinado (cabeçalhos `x-unkvoid-timestamp` e `x-unkvoid-signature`, assinatura
sobre `ts\nMÉTODO\ncaminho\ncorpo`, janela de 300 s — como o `kick` de hoje):

| rota | corpo | resposta |
|---|---|---|
| `POST /rooms/:channel/kick` (já existe) | `{ "userId": "user:12", "to"?: "<ulid do destino>", "by"?: "Edsu" }` — com `to`, é mover (acima); o canal de origem e `to` fora de `[a-z0-9]{26}` ou `by` que não é string de até 64 → 422 | `{ kicked: n }` |
| `POST /broadcast` | `{ channel, event, data }` — o tempo real do Laravel | `{ delivered: n }` (quantos sockets inscritos receberam) |
| `POST /rooms/:channel/mute` | `{ "userId": "user:12", "muted": true }` — pausa/retoma o producer `mic` daquela conta | `{ muted: n }` |
| `POST /kick`, `POST /mute` | o mesmo corpo mais `rooms: ["<ulid>", …]`, as salas em que agir (obrigatória, não vazia; cada uma no formato de sala, senão 422). É o que expulsar, banir e mutar no servidor usam, com os canais de voz **daquele servidor**: o Laravel não precisa saber em qual a pessoa está (a presença esconde quem está na carência e some quando o SFU demora), sai numa chamada só, e nunca alcança a voz de outro servidor — a conta tem uma sessão no SFU inteiro, e ela pode estar lá | `{ kicked: n }`, `{ muted: n }` |

**Ordem de deploy: o SFU sobe antes do web, sem exceção** (desde o PR #52, 09/10/2026). Com o web
na frente, o `/kick` e o `/mute` com salas dão 404 no SFU antigo e banir não derruba ninguém da
voz; a presença antiga não traz quem está na carência; e a claim `muted` do token é ignorada.
| `GET /presence` | corpo vazio | `{ rooms: { "<room>": [ { sub, name, sources: ["mic","screen"], muted, deafened, reconnecting } ] } }` — quem está na carência de reconexão **vem junto**, com `reconnecting: true`: o lugar dele ainda é dele |

`consumePlain` devolve também `ssrc` do consumer: o receptor nativo (`PlainReceiver`) separa os
producers de uma mesma porta por SSRC, sem adivinhar pelo primeiro pacote.

E devolve `rtx: { ssrc, payloadType } | null` — o fluxo de retransmissão do consumer
(RFC 4588), quando o codec tem um. É por ele que o receptor nativo (`media::PlainReceiver`,
usado pelo macOS, pelo Windows e pelo Linux) recupera pacote perdido: manda um NACK
(RTCP PT 205, FMT 1) com os números que faltaram, o SFU reenvia pelo `rtx.ssrc` com o
número original nos dois primeiros bytes do payload, e o quadro sai inteiro. Quando a
espera passa de 250 ms o buraco é largado e vai um PLI (PT 206, FMT 1) pedindo keyframe.
Os dois RTCP saem cifrados (SRTCP) pelo mesmo socket e com a mesma chave do `consumePlain`.
Sem `rtx`, o receptor ainda reordena e pede keyframe; só não recebe o reenvio.

**O servidor diz se a tela está chegando nele.** A sala inteira (o dono também, por causa
do "ver o que a sala vê") recebe
`producerReceiving { producerId, receiving }` quando o RTP de um producer começa ou para de
chegar ao SFU — o mediasoup zera a nota ~1,5 s depois do último pacote —, e o `consumePlain`
devolve o estado do momento em `receiving`. É assim que o app de quem assiste separa a tela
parada de quem transmite (`receiving: false`, nada a fazer) do caminho até ele que morreu
(`receiving: true` e nada chegando há 5 s: refaz o transporte de chegada).

**Chave nova troca o transporte.** O `producePlain` e o `consumePlain` reaproveitam o
transporte de RTP puro da pessoa (um de subida, um de chegada) enquanto a `keyBase64` for a
mesma; com outra chave, o SFU fecha o antigo — e com ele os producers ou consumers que
estavam nele, com o `producerClosed` de sempre para a sala — e abre um novo. É assim que o
app refaz o caminho: o `comedia` prende o transporte ao primeiro endereço de onde veio
pacote, e quando o roteador da pessoa troca de endereço (o provedor reconectou, o roteador
reiniciou) tudo o que vem do endereço novo é descartado. O app troca a chave de chegada a
cada retomada do `join` e a de subida quando passa 5 s mandando sem nenhum RTCP de volta.

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

`POST /api/errors` (público, 30 por minuto) recebe `{ version, platform, log }` — `platform` é
`windows`, `macos` ou `linux`, e `log` tem até 20 mil caracteres — e responde 2xx. O site agrupa
pela versão, pelo sistema e pela primeira linha de pânico ou `ERROR` do log. O app manda
(`core_app::logbook`), de meio em meio minuto, o pedaço do log do dia que ganhou um `ERROR` desde
o último envio, sem o nome do usuário da máquina.

Conta:

| rota | corpo | resposta |
|---|---|---|
| `POST /api/auth/register` (público) | `{ email, password, device, refresh? }` | `{ token, refresh_token, expires_at, user }`. O apelido nasce de `User::freeNickname` sobre o e-mail, com `nickname_confirmed: false` |
| `POST /api/auth/login` (público) | `{ email, password, device, refresh? }` | `{ token, refresh_token, expires_at, user }`. Senha errada é 401 com `"E-mail ou senha não conferem."` |
| `POST /api/auth/refresh` (público) | `{ refresh_token }` | o par novo, no mesmo formato; o token de renovação usado morre na hora. Vencido, usado ou de outra coisa: 401 `"Sua sessão terminou. Entre de novo."` |
| `POST /api/auth/app/exchange` (público, `throttle:20,1`) | `{ code, code_verifier }` | `{ token, refresh_token, expires_at, user }`; código de uso único, ligado ao challenge PKCE, vence em 60 s. Código expirado, usado ou verifier errado: 422 em português |
| `POST /api/auth/logout` | `{ refresh_token? }` | 204; derruba o token de acesso da requisição e, se vier, o de renovação da mesma conta |
| `GET /api/me` | — | `{ id, name, email, avatar_url, avatar_uploaded, admin, nickname_confirmed, bio, pronouns, banner_color, custom_status, status, status_until, activity, created_at }`. `status` é efetivo: prazo vencido retorna `online`; `status_until` é o prazo da própria conta ou `null` |
| `PATCH /api/me` | `{ name?, bio?, pronouns?, banner_color?, custom_status?, status?, status_minutes? }` — `name`: 3 a 32 caracteres, `[A-Za-z0-9._]`, único (pode repetir o atual); `bio` ≤ 190; `pronouns` ≤ 40; `banner_color` `#rrggbb`; `custom_status` ≤ 128; `status`: `online`, `idle`, `dnd` ou `invisible`; `status_minutes`: `null` para sempre, ou 15, 60, 480, 1440 ou 4320 minutos. `null` limpa campos opcionais | o mesmo `user`. Mandar `name` confirma o apelido automático (`nickname_confirmed: true`); os campos mudam só quando enviados. O servidor calcula `status_until` e volta o status a `online` quando vence |
| `PUT /api/me/activity` | `{ activity: { name, started_at } \| null }` — nome de 1 a 80 caracteres e data ISO 8601 | `204`. A atividade é efêmera, guardada em cache por 120 s e renovada pelo cliente enquanto estiver aberta; `null` limpa imediatamente. Só amizades aceitas e membros do mesmo servidor recebem a atividade; invisível a oculta. O compartilhamento pode ser desligado nas preferências locais |
| `POST /api/me/avatar` | `multipart`, campo `avatar` (jpeg/png/webp, ≤ 2 MB) | o mesmo `user`, com a foto nova; guarda no bucket privado e apaga a foto anterior |
| `DELETE /api/me/avatar` | — | o mesmo `user` (200, não 204): tirar a foto enviada faz voltar a valer a do Google, e o app precisa do link novo |

**Login Google no app.** O núcleo gera `code_verifier` aleatório (43–128 caracteres), envia
`code_challenge = BASE64URL(SHA256(verifier))` e `code_challenge_method=S256` em
`/oauth2/app`, junto da porta local e do `state`. O callback volta somente com `code` e `state`;
o app troca o código por `POST /api/auth/app/exchange`. O challenge não é segredo e o verifier
nunca sai do processo. O código é consumido mesmo se o verifier estiver errado.

**A renovação.** Quem manda `refresh: true` (o núcleo nativo manda sempre) recebe o par: o
token de acesso vence em **24 h** (`expires_at`) e o de renovação em **60 dias**, e esse só
serve para trocar o par em `/api/auth/refresh` — como `Bearer` ele não abre rota nenhuma. O login
do Google faz o mesmo com `refresh=1` em `/oauth2/app`: o `refresh_token` volta na porta local ou
no `unkvoid://` ao lado do `token` (sem PKCE), ou no corpo da troca do código (com PKCE).

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

`status` é o estado efetivo da conta (`online`, `idle`, `dnd`, `invisible`); quando `status_until`
vence, o Laravel o troca para `online` e publica `UserStatusUpdated`. Em `MemberResource`, nos
dois lados de `FriendResource`, nas pessoas de mensagens/conversas e na presença do SFU,
`invisible` é sempre exposto como `offline`.

Servidores:

| rota | corpo | resposta |
|---|---|---|
| `GET /api/servers` | — | `[ { id, name, owner_id, icon_url, last_accessed_at, unread } ]`, do último acesso à voz mais recente para o mais antigo; nunca acessado vai para o fim, pela data em que entrou no servidor. `unread` é a soma das não lidas dos canais que a pessoa vê (abaixo) |
| `POST /api/servers` | `{ name }` | `ServerResource` (cria `@everyone`, `#geral` texto, `Geral` voz) |
| `GET /api/servers/{server}` | — | ver "árvore" abaixo |
| `PATCH /api/servers/{server}` | `{ name }` | `ServerResource` (`MANAGE_SERVER`) |
| `DELETE /api/servers/{server}` | — | 204 (dono) |
| `POST /api/servers/{server}/invite` | — | `{ invite_code }` (regenera; `CREATE_INVITE`) |
| `GET /api/invites/{code}` | — | prévia autenticada `{ server: { id, name, icon_url }, members, inviter? }`; inválido, vencido ou pausado → 404 em português |
| `POST /api/invites/{code}` | — | `ServerResource` (entra; banido → 403; convites pausados → 403; convite vencido ou sem usos → 404). Vale o código de um convite de pessoa (abaixo) ou o `invite_code` antigo do servidor; só a entrada nova conta como uso |
| `GET /api/servers/{server}/invites` | — | `[ { id, code, url, uses, max_uses, expires_at, channel: {id,name}?, creator: {id,name,avatar_url}?, created_at } ]`, só os que ainda deixam entrar, do mais novo para o mais antigo (`MANAGE_SERVER`) |
| `POST /api/servers/{server}/invites` | `{ channel_id?, max_age?, max_uses? }` — `max_age` em segundos ∈ {0, 1800, 3600, 21600, 43200, 86400, 604800} (padrão 604800; 0 = nunca vence), `max_uses` ∈ {0, 1, 5, 10, 25, 50, 100} (0 = sem limite); o canal tem de ser do servidor | o convite (201), com código de 8 letras e números. `CREATE_INVITE`; pausados → 403 |
| `DELETE /api/invites/{invite}` | — | 204. Quem criou, ou `MANAGE_SERVER` |
| `POST` / `DELETE /api/servers/{server}/invites/pause` | — | 204. Pausa e retoma todos os convites do servidor (`MANAGE_SERVER`) |
| `POST /api/servers/{server}/leave` | — | 204 (dono → 403) |
| `POST /api/servers/{server}/icon` | `multipart`, campo `icon` (jpeg/png/webp, ≤ 2 MB) | `ServerResource` (`MANAGE_SERVER`); guarda no mesmo bucket privado e apaga o arquivo antigo |
| `DELETE /api/servers/{server}/icon` | — | 204 (`MANAGE_SERVER`); volta ao ícone padrão |
| `GET /api/servers/{server}/audits` | — | as 50 entradas mais recentes do histórico do servidor (`VIEW_AUDIT_LOG`) |

O `url` de um convite abre `GET /convite/{code}` no site (sem login, 30 por minuto por IP): a quem
recebeu, mostra nome, ícone e número de membros do servidor, quem convidou, e como entrar pelo app.
Vencido, pausado ou inexistente mostra só o motivo, sem dizer de que servidor era.

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
  "invites_paused": false,   // POST/DELETE /api/servers/{server}/invites/pause
  "roles": [ { "id": 1, "name": "@everyone", "color": null, "position": 0, "permissions": 31552, "is_everyone": true, "hoist": false, "mentionable": false } ],
  "channels": [ { "id": "01j7…", "name": "geral", "type": "text", "parent_id": null, "topic": null, "position": 0, "user_limit": null,
                  "unread": 3,            // mensagens de outras pessoas depois da minha marca de leitura; 0 em categoria
                  "permissions": 31552,   // as MINHAS efetivas neste canal
                  "overwrites": [ { "target_type": "role", "target_id": 1, "allow": 0, "deny": 256 } ] } ],   // só com MANAGE_ROLES
  "members": [ { "user_id": 12, "name": "Edsu", "avatar_url": null, "nickname": null, "role_ids": [1, 3],
                 "server_mute": false, "server_deaf": false, "is_owner": true,
                 "bio": null, "pronouns": null, "banner_color": null, "custom_status": null,   // o cartão de perfil
                 "member_since": "…", "joined_at": "…", "activity": { "name": "Arena", "started_at": "…" } } ],   // atividade pode ser null
  "voice": { "01j7…": [ { "user_id": 12, "name": "Edsu", "sources": ["mic"], "muted": false, "deafened": false } ] },   // do SFU (/presence), cache 3 s
  "bans": [ { "user_id": 40, "name": "Fulano", "reason": "…", "banned_by": 12, "created_at": "…" } ] }   // só com BAN_MEMBERS; banned_by null se quem baniu apagou a conta
```
Canais que eu não tenho `VIEW_CHANNEL` **não aparecem**.

**Categoria** é um canal com `type: "category"`: agrupa os de texto e de voz que apontam para
ela por `parent_id`, não tem chat, voz nem `user_limit`, e não entra em outra categoria. A
visibilidade dela é a de qualquer canal (as próprias sobrescritas); as dos filhos **não**
sincronizam com a dela — cada canal tem as suas, como antes. Apagar a categoria devolve os
canais dela à raiz (`parent_id: null`). A ordem é `position`, dentro e fora de categoria; o app
agrupa por `parent_id`.

Membros (`{user}` é id de usuário):

| rota | corpo | regra |
|---|---|---|
| `PATCH /api/servers/{server}/members/{user}` | `{ nickname?, role_ids?, server_mute? }` | `MANAGE_ROLES` para cargos (só cargos abaixo do meu top, e só com permissões que eu tenho), `MUTE_MEMBERS` para o bool (chama `POST /rooms/:channel/mute` no SFU se a pessoa estiver em voz), apelido próprio sempre. `server_deaf` existe na tabela mas ainda não tem escrita |
| `DELETE /api/servers/{server}/members/{user}` | — | `KICK_MEMBERS` + hierarquia; derruba da voz pelo `POST /kick` do SFU com os canais de voz **deste** servidor (o `mute` do servidor e o banimento idem): numa chamada só, sem depender da presença, e sem tocar a voz de outro servidor |
| `GET /api/servers/{server}/bans` | — | `BAN_MEMBERS` |
| `POST /api/servers/{server}/bans/{user}` | `{ reason? }` | `BAN_MEMBERS` + hierarquia; remove membro, derruba da voz |
| `DELETE /api/servers/{server}/bans/{user}` | — | `BAN_MEMBERS` |

Sons do soundboard (até 8 por servidor; o arquivo mora no bucket privado, como o ícone):

| rota | corpo | resposta e regra |
|---|---|---|
| `GET /api/servers/{server}/sounds` | — | `[ { id, name, emoji, volume, url, user_id, created_at } ]`, do mais antigo para o mais novo; `url` é pré-assinada e vence em 2 h. Qualquer membro |
| `POST /api/servers/{server}/sounds` | `multipart`: `sound` (mp3, ogg ou wav, ≤ 512 KB), `name` (1–32), `emoji?` (≤ 32), `volume?` (0–100, padrão 100) | o som criado (201). `MANAGE_EXPRESSIONS`; o nono → 422 em `sound` |
| `DELETE /api/sounds/{sound}` | — | 204. `MANAGE_EXPRESSIONS` no servidor do som; o arquivo sai do bucket depois da linha |

Tocar não passa pelo servidor: o app baixa o som pela `url` e o mistura na própria voz, então
todo mundo na chamada ouve pelo áudio que já sobe. O app corta em 5 s.

Cargos:

| rota | corpo |
|---|---|
| `POST /api/servers/{server}/roles` | `{ name, color?, permissions }` (`MANAGE_ROLES`; posição = top do criador − 1, mínimo 1) |
| `PATCH /api/roles/{role}` | `{ name?, color?, permissions?, position?, hoist?, mentionable? }` (`@everyone`: só `permissions`). `hoist` põe quem tem o cargo num grupo próprio na lista de membros (sem nenhum cargo assim, a pessoa fica em "Online"); `mentionable` deixa qualquer um @mencionar o cargo |
| `DELETE /api/roles/{role}` | — (`@everyone` → 422) |

Canais:

| rota | corpo |
|---|---|
| `POST /api/servers/{server}/channels` | `{ name, type: text\|voice\|category, topic?, user_limit?, parent_id? }` (`MANAGE_CHANNELS`; `parent_id` tem de ser categoria do mesmo servidor, e categoria não leva `parent_id` nem `user_limit` → 422) |
| `PATCH /api/channels/{channel}` | `{ name?, topic?, position?, user_limit?, parent_id? }` (`parent_id: null` tira da categoria) |
| `DELETE /api/channels/{channel}` | — (último canal de texto → 422; categoria solta os canais dela) |
| `PUT /api/channels/{channel}/overwrites/{type}/{id}` | `{ allow, deny }` (`MANAGE_ROLES`; `type` = `role`\|`member`; `id` = id do cargo ou id do usuário) |
| `DELETE /api/channels/{channel}/overwrites/{type}/{id}` | — |

Mensagens:

| rota | corpo | resposta |
|---|---|---|
| `GET /api/channels/{channel}/messages?before={id}` | — | 50 mais recentes antes de `before`, ordem crescente: `[ { id, channel_id, user: {id,name,avatar_url}, type, body, files, reply_to, edited_at, created_at } ]`. **Sem `before`, marca o canal como lido** até a mensagem mais recente — é o canal aberto na tela; paginar para trás não marca |
| `POST /api/channels/{channel}/messages/read` | `{ message_id? }` | 204. Marca como lido até `message_id` (sem ele, até a mais recente); a marca nunca volta. É o que o app chama quando a mensagem cai com o canal **já aberto**, e o "Marcar como lido" do menu. `VIEW_CHANNEL`; categoria → 403 |
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
chat ao lado do palco de quem está naquela voz. Categoria não tem: 403.

**Não lidas.** `unread` de um canal é quantas mensagens **de outras pessoas** chegaram depois da
marca de leitura da pessoa naquele canal (`channel_reads`: uma linha por pessoa e canal com o id
da última mensagem lida; sem linha, nunca leu e tudo conta). As próprias mensagens nunca contam.
O `unread` do servidor (`GET /api/servers`) soma só os canais que a pessoa vê: canal oculto não
acende a bolinha. O tempo real não traz o número: o app soma o `MessageSent` que chega em canal
que não está aberto e zera ao abrir (o `GET` marca) — ao reconectar, a árvore traz o número certo.

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
| `POST /api/channels/{channel}/voice/token` | — | `{ token, url, expires_in: 60 }` (`CONNECT` no canal de voz; `user_limit` cheio → 403; grava `channel_accesses`). **Quem o SFU ainda conhece na sala** (presença fresca, com o socket de pé ou na carência, `reconnecting`) está reconectando: não passa de novo por `CONNECT` nem pelo limite, e não abre acesso novo — cair da rede num canal trancado ou cheio não tira ninguém de lá. A medida é o SFU, nunca o acesso em banco: um `left` perdido não vira lugar. Quem está na carência **conta no limite**: o lugar de quem caiu fica guardado até ele voltar ou a carência acabar. Quem acabou de ser **movido para cá** entra sem `CONNECT` e sem contar o limite durante 60 s; **mover alguém daqui revoga o passe** que ele tinha para cá. Quem acabou de ser **movido daqui** é recusado por 60 s (403 "Você acabou de ser movido para outro canal."), salvo se foi movido de volta para cá nesse meio-tempo |
| `PATCH /api/channels/{channel}/voice/members/{user}` | `{ channel_id }` (o ULID do destino) | 204: **move** a pessoa para outra voz (regra do Discord). Quem move: `MOVE_MEMBERS` na origem **e** no destino, `CONNECT` no destino, e hierarquia sobre a pessoa. Quem é movido: **não** precisa de `CONNECT` e o `user_limit` do destino é ignorado; só tem de **ver** o destino (403); ela tem de estar na voz da origem agora (404); o destino tem de ser canal de voz do mesmo servidor e outro que a origem (422). O Laravel grava o passe de 60 s e chama o `kick` do SFU com `to` e `by`; o resto é o SFU avisar `moved` e o app do movido entrar sozinho (acima). `VoiceStateUpdated left/joined` saem pelos webhooks como sempre |
| `DELETE /api/channels/{channel}/voice/members/{user}` | — | 204 (`MOVE_MEMBERS` + hierarquia; `kick` no SFU) |

Webhook (assinado, sem Sanctum): `POST /api/sfu/events` — corpo acima. `joined` fecha
qualquer acesso aberto do mesmo usuário no mesmo canal e abre um novo (com `sfu_ip`);
`left` fecha o aberto. Os dois retransmitem `VoiceStateUpdated`.

Tempo real (as duas rotas que fazem o chat e a presença andarem sem o Reverb):

| rota | corpo | resposta |
|---|---|---|
| `POST /api/sfu/authorize` (assinada, sem Sanctum) | `{ sub: "user:12", channel: "channel.01j7…", at }` | `{ allowed, name, status }` **sem o `data`** — é o único JSON da API que sai cru, porque quem lê é o SFU. Para os demais, `invisible` vira `offline` |
| `POST /api/sfu/session` (`auth:sanctum`) | — | `{ token, url, expires_in: 60 }` — o token do `identify`, no mesmo formato do de voz, mas com as claims `{ sub, name, exp }` e **sem `room` nem `can`** |

## Amigos

Uma linha por par, na direção em que o pedido foi feito, com `status` `pending`,
`accepted` ou `blocked`. Bloquear não cria uma segunda linha: é a mesma mudando de
situação, e quem bloqueou passa a ser o `requester`.

| rota | corpo | resposta |
|---|---|---|
| `GET /api/friends` | — | `[FriendResource]` — os dois lados vão no recurso, porque a interface precisa saber se mostra "aceitar" ou "aguardando"; a pessoa inclui `activity: { name, started_at } \| null`, `created_at` e `banner_color` |
| `GET /api/friends/{user}/mutuals` | — | `{ mutual_friends, mutual_servers: [{ id, name, icon_url }] }`; autenticada, só para amizade aceita ou servidor em comum; bloqueios e pessoas sem relação retornam 404 |
| `POST /api/friends` | `{ email }` | `FriendResource` (throttle 20/min). E-mail que não existe responde o mesmo que e-mail não encontrado, para a busca não virar lista de quem tem conta |
| `POST /api/friends/{user}` | — | `FriendResource` (throttle 20/min); pede amizade a uma pessoa encontrada no app |
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
| `GET /api/dm` | — | uma linha por conversa, a da mensagem mais recente primeiro: `[ { user: {id,name,avatar_url,status,activity,created_at,banner_color}, last: { id, body, created_at, mine }, unread } ]` |
| `GET /api/dm/{user}?before={id}` | — | as 50 mais recentes antes de `before`, ordem crescente: `[DirectMessageResource]`. **Marca como lidas** as que chegaram para quem pediu: é assim que o app zera o `unread` |
| `POST /api/dm/{user}` | `{ body }` (1–2000) | `DirectMessageResource` (throttle 60/min) |
| `POST /api/dm/{user}/read` | — | `204`. Marca como lidas as que chegaram daquela pessoa — é o que o app chama quando a mensagem cai com a conversa **já aberta**, porque aí não houve `GET` para marcar |
| `PATCH /api/dm/{directMessage}` | `{ body }` | só o autor; grava `edited_at` |
| `DELETE /api/dm/{directMessage}` | — | 204, só o autor (soft delete: some da conversa, fica no banco) |

```json
{ "id": 12, "body": "oi", "created_at": "…", "edited_at": null, "mine": true,
  "sender": { "id": 2, "name": "Edsu", "avatar_url": null, "created_at": "…", "banner_color": null } }
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

O `subscribe` devolve a presença como `{ id, name, status }`, e o SFU emite
`presence.joining { id, name, status }` e `presence.leaving { id }` para os outros inscritos na
primeira e na última conexão de cada pessoa naquele canal. O status é público; `invisible` chega
como `offline`.

**O canal `releases` é público.** Qualquer socket o assina, sem `identify` e sem o Laravel
autorizar, e ele não tem presença: o `subscribe` devolve só `{ channel }`, e ninguém entra nem
sai da lista. Só recebe o que o Laravel publica. É por ele que a versão nova chega ao app antes
de a pessoa se identificar para o tempo real dos servidores, inclusive a quem entrou sem conta.

O que o Laravel publica sai por `POST /broadcast` assinado, um pedido por canal. Falha do
SFU não desfaz nada: a escrita já está no banco, a resposta HTTP sai normal e o erro fica no
log do canal `sfu`. Como nada é reentregue depois de uma queda, ao reconectar o app busca de
novo pela API os servidores, a árvore aberta, amigos, a lista de conversas, e as 50 mensagens
mais recentes do canal e da conversa abertos, emendando com o que já estava na tela.

| canal | quem entra | eventos |
|---|---|---|
| `channel.{ulid}` | `VIEW_CHANNEL` | texto: `MessageSent { message }`, `MessageUpdated { message }`, `MessageDeleted { id, channel_id }` · voz: `VoiceStateUpdated { channel_id, user_id, name, event: joined\|left }` (no canal da própria voz, para canal oculto não vazar quem está nele; o app assina o canal de cada voz que enxerga) · `VoiceMuteUpdated { channel_id, user_id, name, muted, deafened }` (publicado pelo **SFU**, não pelo Laravel: é o `voiceState` de quem está na voz) |
| `server.{id}` | membro | `ServerUpdated { server_id }` (qualquer mudança de estrutura: o app refaz o `GET`) |
| `releases` | qualquer socket, sem `identify` | `ReleasePublished { version, platform }` (a cada `POST /api/releases`: o app compara com a própria versão e plataforma, anota a versão na configuração e baixa calado até mostrar o botão verde) |
| `user.{id}` | o próprio | `FriendshipUpdated { friendship, removed }` (`FriendResource`, nos canais dos **dois** lados) · `UserStatusUpdated { user_id, status }` (no próprio canal e nos canais dos amigos aceitos; invisível publica `offline`) · `MemberRemoved { server_id, reason: kicked\|banned }` · `DirectMessageCreated { message, recipient }`, `DirectMessageUpdated { message, recipient }`, `DirectMessageDeleted { id }` (nos canais dos **dois** lados da conversa; `message` é o `DirectMessageResource` sem o `mine`) |
| `friend.{id}` | o próprio e seus amigos aceitos | `UserStatusUpdated { user_id, status }` · `ActivityUpdated { user_id, activity }`; este canal separado não expõe mensagens diretas |
| `server.{id}` | membro | `ServerUpdated { server_id }` (qualquer mudança de estrutura: o app refaz o `GET`) · `UserStatusUpdated { user_id, status }` (status público de um membro) · `ActivityUpdated { user_id, activity }` |

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

O app é um só, em Rust com egui (`native/apps/unkvoid`), no desenho do Discord: trilho de
servidores | canais (ou conversas, na Home) | centro | membros. A janela não tem moldura do
sistema: a barra de cima é desenhada pelo app, arrasta, maximiza no clique duplo e a borda
redimensiona.

- Entrada: "Entrar com o Google" (`/oauth2/app?state=&port=&refresh=1&code_challenge=&code_challenge_method=S256`, de volta com um código por uma porta em
  `127.0.0.1`, trocado no endpoint PKCE) e e-mail e senha, com "Registre-se" alternando para criar conta. O token do Sanctum
  fica cifrado no disco, com a chave no chaveiro do sistema (`shared/storage`). Com token válido
  (`GET /api/me`), abre o hub.
- A sala por código continua: sem conta entra pelo `join` sem token, e logado pelo token de
  `POST /api/rooms/{code}/token`.
- Nos formulários, o campo recusado fica com o rótulo vermelho e a mensagem ao lado dele; o que
  não é de campo (credencial errada, 429) fica na linha geral.
- Com `nickname_confirmed: false`, um aviso que não fecha pede o apelido até o `PATCH /api/me`
  dar certo; dá para sair da conta por ele. "Minha conta" troca o apelido pelo mesmo `PATCH`.
- Trilho: a Home (conversas diretas, com a soma das não lidas), um ícone por servidor (o
  `icon_url` ou as iniciais) e o "+" (criar servidor ou entrar por convite, que aceita o link
  inteiro). A pílula branca na borda diz o aberto, o do cursor e o que tem não lidas
  (`unread` do `GET /api/servers`).
- Coluna do servidor: o nome abre o menu (convidar, criar canal, criar categoria, recolher
  categorias, copiar ID, sair do servidor). Os canais vêm de `ServerTree::sections` (núcleo): os
  soltos na raiz primeiro, depois cada categoria pela posição, texto antes de voz; categoria
  recolhe no clique. Canal com não lidas fica em negrito com a pílula; o que `@everyone` não vê
  leva cadeado. Embaixo de cada canal de voz, quem está nele (o `voice` da árvore).
- Chat: mensagens agrupadas como no Discord (mesma pessoa, mesmo dia, menos de 7 minutos), o
  nome na cor do cargo mais alto, divisor de dia, "(editado)", a citação da resposta e as
  imagens. No hover, copiar, editar (a própria) e apagar (a própria, ou com `MANAGE_MESSAGES`).
  Rolar até o alto busca a página anterior (`?before=`) sem a tela pular. Enter envia,
  Shift+Enter quebra a linha. O começo do canal mostra "Boas-vindas a #canal!".
- Membros: online agrupados pelo cargo mais alto com a cor dele, offline no fim, apagados
  (`members::group` do núcleo); coroa no dono; clique direito com "Mensagem" e "Copiar ID".
- Home: Amigos (todos, pendentes com aceitar e recusar, adicionar pelo e-mail) e a conversa
  direta aberta.
- Avisos no canto de baixo, que somem em 3,5 s (6 s o de erro). Versão nova baixada: o botão
  verde na barra de cima reinicia e instala.

**Voz, tela e câmera chegam no app novo na fase 2** (desenho em `docs/VOZ.md`). O que vale
para eles, igual ao app de antes:

- O clique no canal de voz vale na hora: a pessoa já aparece na lista do canal e a barra de voz
  mostra "Conectando…" enquanto o token, o SFU e o microfone acontecem por trás. Se a entrada
  falhar, ela sai da lista. Entrar liga o microfone **aberto**, salvo "Silenciar ao entrar".
- Microfone e câmera sobem pelo núcleo em RTP puro (`producePlain`), como a tela, que só se
  compartilha de dentro da voz.
- Áudio de `screenAudio` chega **mudo**. `mic` toca direto. `camera` vira cartão pequeno.
- Cada pessoa da voz tem volume e mudo locais (guardados por conta).
- Variáveis de ambiente do app, para calibrar e diagnosticar: `UNKVOID_SERVER` (outro Laravel),
  `UNKVOID_ENCODER=cpu` (pula o encoder da placa), `UNKVOID_DECODER=cpu` (assiste sem o DXVA, no
  Windows), `UNKVOID_ABR=off` (taxa fixa, sem acompanhar a perda), `UNKVOID_CAPTURE=x11|portal`
  (força a captura do Linux).

## Rodar tudo local (para testar antes de subir)

Três processos e o app, todos na mesma máquina ou na mesma rede:

```bash
# 1. Laravel (API e site). O chat e a presença saem pelo SFU, não há mais Reverb
cd web && composer dev            # serve em :8000, fila, logs, vite

# 2. SFU (mídia). O segredo tem de ser o mesmo SFU_SECRET do web/.env
cd sfu && pnpm run build && SFU_SECRET=<o mesmo do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js

# 3. App apontando para o Laravel local (o SFU vem do GET /api/config)
cd native && UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid
```

Duas máquinas na mesma rede: troque `127.0.0.1` pelo IP da máquina que roda os
servidores em `APP_URL` e `SFU_PUBLIC_URL` (`web/.env`), suba o SFU com
`SFU_HOST=0.0.0.0 SFU_ANNOUNCED_ADDRESS=<IP>`, e o Laravel com
`php artisan serve --host=0.0.0.0`. No WSL2 a rede só enxerga o UDP do SFU com
`networkingMode=mirrored` no `.wslconfig`.

O app instalado aponta para outro Laravel com `UNKVOID_SERVER=http://<IP>:8000` no ambiente de
quem o abre.
