# SFU

O relé de mídia do Unkvoid. Recebe **um** fluxo de quem transmite e entrega para **N** quem
assiste, sem decodificar nem recomprimir nada no caminho — é isso que deixa o jogo rodando
enquanto a tela é compartilhada.

Node 22+ e [mediasoup](https://mediasoup.org). Roda na VPS, sob pm2.

Ele não decide permissão. O Laravel decide quem pode o quê e assina um token de 60 s; aqui só
se confere a assinatura.

## Como uma chamada acontece

```
app  --WebSocket /sfu-->  SFU          sinalização (quem entra, quem produz, quem consome)
app  --UDP/RTP------->    SFU  --->  N espectadores      a mídia
Laravel --HTTP assinado-> SFU          expulsar, silenciar, quem está na sala
SFU  --webhook---------> Laravel       avisa quem entrou e saiu
```

## As pastas

| Pasta | O que tem |
|---|---|
| `src/app.ts` | monta o Express e o WebSocketServer no mesmo servidor HTTP; o heartbeat e o teto de conexões por IP |
| `src/server.ts` | sobe os workers e faz o `listen` |
| `src/Config/` | tudo que vem do ambiente, os codecs e as portas |
| `src/Routers/` | `HttpRouter` (as 5 rotas HTTP) e `WebSocketRouter` (as 19 ações do WebSocket) |
| `src/Http/Controller/` | um por recurso, mais `HealthController` e `RoomController` para o HTTP |
| `src/Http/Request/` | valida a entrada de cada ação, no molde do FormRequest do Laravel |
| `src/Services/` | o coração: `Room`, `Peer`, `RoomRegistry`, `Kernel`, `Signature`, `Webhook` |
| `src/Http/Middleware/` | `VerifySignature` (a assinatura HMAC das chamadas do Laravel) e `Cors` |
| `src/Exceptions/` | `ApiException` e filhas; o status HTTP mora na exceção |
| `src/Enums/` | `Action` (as ações do WebSocket) e `Source` (mic, tela, câmera) |

### Os Services, um por um

| Arquivo | O que faz |
|---|---|
| `RoomRegistry.ts` | os workers do mediasoup, o router novo no worker mais vazio, e o worker morto que renasce |
| `Room.ts` | a sala: quem está dentro, a carência de 30 s ao cair, uma sessão por conta, e os routers dela (um por worker que ela usa, ligados por `pipeToRouter`) |
| `Peer.ts` | uma pessoa: seus transportes, producers e consumers |
| `Kernel.ts` | despacha cada ação do WebSocket para o controller, como o Kernel do Laravel |
| `Signature.ts` | confere o token HMAC do `join` e a assinatura das chamadas do Laravel |
| `Webhook.ts` | avisa o Laravel (`joined`, `left`) sem nunca segurar o `join` |

## As rotas

### HTTP (Express) — `src/Routers/HttpRouter.ts`

| Rota | Quem chama | Assinada |
|---|---|---|
| `GET /health` | o app, antes de entrar | não |
| `GET /presence` | o Laravel | sim |
| `POST /rooms/:room/kick` | o Laravel | sim |
| `POST /rooms/:room/mute` | o Laravel | sim |
| `POST /broadcast` | o Laravel, para o tempo real | sim |

A assinatura é HMAC sobre o **corpo cru**. Por isso o `express.json` guarda os bytes
originais em `rawBody`: reserializar o objeto troca espaços e ordem de chaves, e a conta não
bate mais.

### WebSocket — `src/Routers/WebSocketRouter.ts`

Tudo o mais é WebSocket em `/sfu`, e **não** passa pelo Express. São 20 ações: `join`,
`leave`, `ping`, `removePeer`, `identify`, `subscribe`, `unsubscribe`, `createTransport`,
`connectTransport`, `produce`, `producePlain`, `pauseProducer`, `resumeProducer`,
`closeProducer`, `consume`, `consumePlain`, `pauseConsumer`, `resumeConsumer`, `closeConsumer`,
`voiceState`.

Abertas sem estar numa sala: `join`, `ping` e as três do tempo real (`identify`, `subscribe`,
`unsubscribe`). As outras exigem sessão. O app nativo usa as de RTP puro (`producePlain`,
`consumePlain`); `createTransport`, `produce` e `consume` são do WebRTC do app Tauri de antes.

## Rodar local

O segredo tem de ser **o mesmo** `SFU_SECRET` do `web/.env`, senão nenhum token é aceito.

```bash
pnpm install
pnpm run dev      # nodemon: recompila e reinicia a cada alteracao em src/
```

O `pnpm run dev` lê o `sfu/.env` pelo `--env-file` nativo do Node, então não precisa passar
`SFU_SECRET` na linha. Sem watch:

```bash
pnpm run build
SFU_SECRET=<o do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js
```

Duas máquinas na mesma rede: `SFU_HOST=0.0.0.0 SFU_ANNOUNCED_ADDRESS=<o IP>`.

## CORS

Só importa para a interface React rodando no navegador (`npm run dev` em
`native/apps/desktop`): os apps nativos não são navegador. Em produção quem põe o cabeçalho é o
nginx (`location = /health`). Local **não há nginx no caminho**: a interface está em
`http://localhost:1420` e fala direto com a porta 3000, e sem o cabeçalho o navegador recusa a
resposta.

```bash
CORS_URL=http://localhost:1420,https://unkvoid.com
```

Sem a variável vale `*`. A assinatura HMAC não é afetada: CORS é regra de navegador, e
quem chama as rotas assinadas é o Laravel, de servidor para servidor.

## Ambiente

| Variável | Para quê | Padrão |
|---|---|---|
| `SFU_SECRET` | o segredo do HMAC, igual ao do `web/.env` | — (obrigatório) |
| `SFU_LARAVEL_URL` | para onde vai o webhook | — |
| `CORS_URL` | origens que podem chamar o HTTP, separadas por vírgula | `*` |
| `SFU_HOST`, `SFU_PORT` | onde escuta | `127.0.0.1`, `3000` |
| `SFU_PATH` | o caminho do WebSocket | `/sfu` |
| `SFU_ANNOUNCED_ADDRESS` | o IP que o SFU anuncia para o RTP | — |
| `SFU_MEDIA_PORT` | a porta base da mídia | `40000` |
| `SFU_PLAIN_PORT`, `SFU_PLAIN_PORTS` | as portas de RTP puro | `41000`, 8 por worker (64 na VPS) |
| `SFU_WORKERS` | quantos workers do mediasoup | os núcleos da máquina (na VPS, 3: núcleos menos um) |
| `SFU_PEERS_PER_ROUTER` | quantas pessoas cabem num router antes de a sala abrir outro, noutro worker | `10` |
| `SFU_HEARTBEAT_MS` | de quanto em quanto pergunta se o socket vive | `15000` |
| `SFU_CONNECTIONS_PER_MINUTE` | teto de conexões novas por IP | — |
| `SFU_APP_VERSION` | o que o `/health` devolve | — |

## Verificar

```bash
pnpm run build        # tsc
pnpm run check        # eslint + check.mjs: o contrato contra um SFU no ar (SFU_SECRET e SFU_CHECK_URL)
SFU_SECRET=<o do servidor> node --test check-realtime.mjs   # o tempo real contra um SFU no ar
```

O `check.mjs` sobe SFUs próprios nas portas 3197-3199 para os cenários que precisam de outra
configuração (webhook, heartbeat, worker morto), e nunca mexe no que já está no ar.

## Publicar

O pm2 roda o SFU do clone da VPS, `/var/www/projects/unkvoid/sfu` (`cwd: __dirname` no
ecosystem). O `deploy-sfu.yml` está pausado (só disparo à mão), então o deploy é ali mesmo:

```bash
ssh vps
cd /var/www/projects/unkvoid && git fetch && git merge --ff-only origin/<branch>
cd sfu && pnpm install --frozen-lockfile --prod=false && pnpm run build   # sem `| tail`: o tsc emite o JS mesmo com erro
pm2 restart sfu
```

Mudou alguma variável do `ecosystem.config.cjs` (ex.: `SFU_WORKERS`)? `pm2 restart` mantém o
ambiente antigo: é `pm2 delete sfu && pm2 start ecosystem.config.cjs --only sfu && pm2 save`.
O `SFU_ANNOUNCED_ADDRESS` vem de um `.env` fora do repositório (o `sfu/.env` do clone ou o
`/var/www/projects/sfu/.env`); sem ele a mídia anuncia `127.0.0.1` e toda chamada fica preta.

Reiniciar derruba quem está em chamada por alguns segundos: o SFU novo não tem as sessões
antigas, então o app reconecta sozinho, com espera sorteada, entra de novo e republica o que
transmitia.

## Duas coisas que não são óbvias

**O heartbeat não é enfeite.** Um socket meio aberto — tampa do notebook fechada, Wi-Fi
trocado por 4G — nunca manda FIN nem RST. Sem o ping de 15 s, o `close` não dispara, a pessoa
fica eternamente ativa na sala, o router do mediasoup nunca é devolvido e as portas de RTP
puro não voltam. É um vazamento que acaba batendo no `max_memory_restart` do pm2 e derrubando
a chamada de todo mundo.

**Worker morto leva só as salas dele.** O mediasoup registra `SIGINT` e `SIGTERM` para
fechar os workers, e um listener basta para o Node não sair mais no sinal: o processo ficava
de pé sem worker nenhum, o `/health` dizia ok e todo `join` dava 500 (`Channel closed`). Hoje o
`server.ts` sai no sinal, o `/health` dá 503 sem worker vivo, e o worker que morre renasce
sozinho: as salas dele fecham com 1012 e cada app reconecta num worker vivo.

**Cair não é sair.** Quem perde a sinalização entra numa carência de 30 s com a mídia viva, e
pode reconectar sem cair da chamada. A sala vê `peerConnectionLost` na hora e o `peerLeft` só
quando a carência acaba.

## Onde está escrito o resto

- [../docs/CONTRATO.md](../docs/CONTRATO.md) — o contrato entre app, SFU e Laravel: formato do
  token, ações, eventos, webhook
- [../docs/ARQUITETURA.md](../docs/ARQUITETURA.md) — o mapa das três peças
- [../docs/REDE.md](../docs/REDE.md), [../docs/UDP.md](../docs/UDP.md) — portas e firewall
