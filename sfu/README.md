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
| `src/Routers/` | `HttpRouter` (as 4 rotas HTTP) e `WebSocketRouter` (as 16 ações do WebSocket) |
| `src/Http/Controller/` | um por recurso, mais `HealthController` e `RoomController` para o HTTP |
| `src/Http/Request/` | valida a entrada de cada ação, no molde do FormRequest do Laravel |
| `src/Services/` | o coração: `Room`, `Peer`, `RoomRegistry`, `Kernel`, `Signature`, `Webhook` |
| `src/Http/Middleware/` | `VerifySignature` (a assinatura HMAC das chamadas do Laravel) e `Cors` |
| `src/Exceptions/` | `ApiException` e filhas; o status HTTP mora na exceção |
| `src/Enums/` | `Action` (as ações do WebSocket) e `Source` (mic, tela, câmera) |

### Os Services, um por um

| Arquivo | O que faz |
|---|---|
| `RoomRegistry.ts` | os workers do mediasoup e em qual deles cada sala mora |
| `Room.ts` | a sala: quem está dentro, a carência de 30 s ao cair, uma sessão por conta |
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

A assinatura é HMAC sobre o **corpo cru**. Por isso o `express.json` guarda os bytes
originais em `rawBody`: reserializar o objeto troca espaços e ordem de chaves, e a conta não
bate mais.

### WebSocket — `src/Routers/WebSocketRouter.ts`

Tudo o mais é WebSocket em `/sfu`, e **não** passa pelo Express. São 16 ações: `join`,
`leave`, `ping`, `removePeer`, `createTransport`, `connectTransport`, `produce`,
`producePlain`, `pauseProducer`, `resumeProducer`, `closeProducer`, `consume`,
`consumePlain`, `pauseConsumer`, `resumeConsumer`, `closeConsumer`.

Só `join` e `ping` são abertas; as outras exigem sessão.

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

Em produção quem põe o cabeçalho é o nginx (`location = /health`), então o SFU nunca
precisou disso. Rodando local **não há nginx no caminho**: o app está em
`http://localhost:1420` e fala direto com a porta 3000, e sem o cabeçalho a webview
recusa a resposta e o app mostra "Servidor sem resposta".

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
| `SFU_PLAIN_PORT`, `SFU_PLAIN_PORTS` | as portas de RTP puro | `41000`, 8 por worker |
| `SFU_WORKERS` | quantos workers do mediasoup | os núcleos da máquina |
| `SFU_HEARTBEAT_MS` | de quanto em quanto pergunta se o socket vive | `15000` |
| `SFU_CONNECTIONS_PER_MINUTE` | teto de conexões novas por IP | — |
| `SFU_APP_VERSION` | o que o `/health` devolve | — |

## Verificar

```bash
pnpm run check        # eslint
pnpm run typecheck    # tsc --noEmit
pnpm run build        # tsc
```

## Publicar

Um push na `main` que toque em `sfu/` dispara o `.github/workflows/deploy-sfu.yml`: ele dá
`git reset --hard origin/main` no clone da VPS, compila e chama o `install.sh` ali mesmo —
sem cópia, porque o pm2 roda desse mesmo diretório (`cwd: __dirname` no ecosystem).

À mão, do notebook:

```bash
./deploy.sh vps
```

O `install.sh` **reinicia na hora**, sem esperar a sala esvaziar: quem está em chamada leva
alguns segundos de tela preta até o app reconectar sozinho (`SfuClient.scheduleReconnect`,
com backoff e jitter) e retomar a sessão pelo `resumeKey`.

## Duas coisas que não são óbvias

**O heartbeat não é enfeite.** Um socket meio aberto — tampa do notebook fechada, Wi-Fi
trocado por 4G — nunca manda FIN nem RST. Sem o ping de 15 s, o `close` não dispara, a pessoa
fica eternamente ativa na sala, o router do mediasoup nunca é devolvido e as portas de RTP
puro não voltam. É um vazamento que acaba batendo no `max_memory_restart` do pm2 e derrubando
a chamada de todo mundo.

**Cair não é sair.** Quem perde a sinalização entra numa carência de 30 s com a mídia viva, e
pode reconectar sem cair da chamada. Só depois disso a sala é avisada.

## Onde está escrito o resto

- [../docs/CONTRATO.md](../docs/CONTRATO.md) — o contrato entre app, SFU e Laravel: formato do
  token, ações, eventos, webhook
- [../docs/ARQUITETURA.md](../docs/ARQUITETURA.md) — o mapa das três peças
- [../docs/REDE.md](../docs/REDE.md), [../docs/UDP.md](../docs/UDP.md) — portas e firewall
