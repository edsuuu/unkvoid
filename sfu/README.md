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
| `src/Routers/` | `HttpRouter` (as 6 rotas HTTP) e `WebSocketRouter` (as 20 ações do WebSocket) |
| `src/Http/Controller/` | um por recurso, mais `HealthController`, `RoomController` e `StatsController` para o HTTP |
| `src/Http/Request/` | valida a entrada de cada ação, no molde do FormRequest do Laravel |
| `src/Services/` | o coração: `Room`, `Peer`, `RoomRegistry`, `Kernel`, `Signature`, `Webhook` |
| `src/Http/Middleware/` | `VerifySignature` (a assinatura HMAC das chamadas do Laravel) e `Cors` |
| `src/Exceptions/` | `ApiException` e filhas; o status HTTP mora na exceção |
| `src/Enums/` | `Action` (as ações do WebSocket) e `Source` (mic, tela, câmera) |
| `e2e/` | a prova ponta a ponta (`pnpm run e2e`): clientes sem tela no protocolo do app nativo, ver abaixo |

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
| `GET /stats` | a prova ponta a ponta e o diagnóstico | sim |
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
pnpm run e2e          # a mídia de ponta a ponta, com SFUs próprios (ver abaixo)
```

O `check.mjs` sobe SFUs próprios nas portas 3197-3199 para os cenários que precisam de outra
configuração (webhook, heartbeat, worker morto), e nunca mexe no que já está no ar.

## Prova ponta a ponta (`pnpm run e2e`)

O `check.mjs` confere o contrato; o `e2e/` confere a mídia. Cada cenário sobe o seu SFU (o
`dist/` de verdade, num processo próprio) e põe na sala pessoas sem tela que falam **o mesmo
protocolo do app nativo**: o `join` com `resumeKey`, o `ping` de 5 s, o `producePlain` e o
`consumePlain` com SRTP `AES_CM_128_HMAC_SHA1_80`, um SSRC por origem, o `H264Payloader`
(STAP-A, FU-A, MTU 1200), o relatório do remetente, o reenvio por NACK, o furo de 5 s, a
recuperação do `recovery.rs` (NACK em até 3 pedidos, buraco largado em 250 ms, PLI) e a volta
depois da queda (`resume`, senão republicar com chave nova). Quem assiste monta os quadros, conta
o que um decodificador mostraria, e o ffmpeg decodifica de verdade o que chegou.

```bash
pnpm run e2e            # todos os cenários (uns 8 minutos)
pnpm run e2e a c        # só os das letras pedidas
E2E_PHP=php8.4 pnpm run e2e e   # o cenário de mover precisa do Laravel (PHP 8.4 e web/vendor)
```

Precisa de `ffmpeg` e `ffprobe` no PATH. O cenário `e` sobe o `web/` num sqlite descartável; sem
PHP 8.4 ou sem `web/vendor` ele é pulado e diz por quê. Os números ficam em
`e2e/out/report.json` (o `run.mjs` resume no fim) e o vídeo que chegou em `e2e/out/*.h264`.

### A mídia

Sem encoder: o quadro-chave é um IDR do ffmpeg (perfil baseline, o `42e01f` do app) por
resolução, guardado em `e2e/.cache`; os quadros P são fatias `P_Skip` montadas à mão, com o
`frame_num` certo, cheias de NAL de enchimento até a taxa pedida (o número de pacotes é o de uma
transmissão de verdade). Cada quadro leva um SEI com o contador e a hora de envio: é por ele que
se prova ordem, buraco e atraso. O som é Opus de verdade (ruído rosa, 20 ms), reconhecido pacote
a pacote pelo conteúdo. O quadro-chave sai quando o servidor pede, com o freio do app (2 s,
dobrando até 4 s) ou na hora (`gate: 'immediate'`, para medir o SFU sozinho), e o GOP de 4 s do
encoder do Windows.

### Os cenários

| Arquivo | O que prova |
|---|---|
| `a-transmitir` | tela 1080p60, câmera 360p30 e microfone chegam a duas pessoas em ordem, sem quadro pulado, sem parada acima de 500 ms, no tamanho e no fps pedidos; o ffmpeg decodifica tudo sem erro |
| `b-entrar-atrasado` | quem entra numa transmissão em curso vê o primeiro quadro em até 1 s só pelo PLI (sem GOP), um por vez, dois a 300 ms, três juntos e logo depois do quadro-chave de outra pessoa (o pior caso do freio); e cada atrasado custa **um** quadro-chave por vídeo a quem transmite |
| `c-perda` | com 5% de perda e jitter na subida e na descida, o NACK/RTX recupera e a imagem não para mais de 1 s (1,5 s com 5% nos dois lados, ≈10% de ponta a ponta) |
| `d-trocas` | trocar a resolução no meio, trocar de tela, parar tudo e recomeçar (o transporte é solto e refeito), duas telas na mesma sala, oito pessoas assistindo, e pausar e retomar (quem assiste e quem transmite) com um quadro-chave só |
| `e-mover` | o `PATCH .../voice/members/{user}` do Laravel no meio da transmissão: `moved` chega, a origem para de receber (nenhum pacote fantasma), o destino vê a tela, o `/stats` não guarda nada da origem, a origem recusa o token por 60 s, e o tempo real manda `left`/`joined` |
| `f-worker` | o worker que morre leva só a sala dele (1012), quem estava nela volta a ver sozinho; a sala espalhada em dois workers (`pipeToRouter`) publica e assiste; e a sala que encolhe devolve o router do outro worker |
| `g-rede` | quem assiste e quem transmite perdem a rede (TCP e UDP) por 10 s e voltam sem reiniciar o app, com o servidor no mesmo número de objetos; 200 ciclos de entrar, transmitir, assistir e sair (um em quatro sem `leave`) voltam ao estado de base, e a memória fica estável |
| `h-carga` | 10 pessoas, 2 telas 1080p30 e 10 câmeras 360p30, todo mundo assistindo tudo: a CPU do SFU (Node e cada worker) num router só e espalhada em dois. Cada pessoa roda numa thread (`ThreadedParticipant`): numa thread só, o próprio harness passava de um núcleo e perdia pacote no socket |

Os testes marcados `# TODO` no relatório reproduzem um defeito do **app nativo** (o SFU não tem
como corrigir); cada um diz o arquivo do app no título. O cliente do harness tem as chaves para
isso: `keyframeRouting: 'shared'` (o pedido de quadro-chave dividido entre tela e câmera),
`pacing: 'native'` (a câmera derrubando o ritmo da tela), `receiverReports` e `extendedReports`
(o RR e o XR DLRR que o app não manda), `gate` e `legacyMoved` (o app que não conhece `moved`).

### Sem `tc netem`

A rede ruim é um proxy UDP (`UdpProxy`) entre cada pessoa e o SFU: perda e atraso nos dois
sentidos, jitter que anda devagar sem embaralhar a fila (como na internet) e uma fração
embaralhada à parte. O cabo puxado é o `cut` dele junto com o do `TcpProxy`, que segura o
WebSocket sem fechar (o socket meio aberto, que não manda FIN nem RST).

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

## Coisas que não são óbvias

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

**O freio de quadro-chave é de meio segundo** (`KEYFRAME_REQUEST_DELAY_MS` no
`ProducerController`, o `keyFrameRequestDelay` do mediasoup). Pedido que chega dentro do freio
espera ele acabar, junto com os outros. Com 1 s, quem entrava logo depois do quadro-chave de outra
pessoa esperava o segundo inteiro mais a viagem do quadro, e passava de 1 s até a primeira imagem
(cenário `b` do `e2e`). Quem protege o encoder de pedido demais é o próprio app (`KeyframeGate`,
2 s); o SFU só junta os pedidos da sala.

**Cair não é sair.** Quem perde a sinalização entra numa carência de 30 s com a mídia viva, e
pode reconectar sem cair da chamada. A sala vê `peerConnectionLost` na hora e o `peerLeft` só
quando a carência acaba.

## Onde está escrito o resto

- [../docs/CONTRATO.md](../docs/CONTRATO.md) — o contrato entre app, SFU e Laravel: formato do
  token, ações, eventos, webhook
- [../docs/ARQUITETURA.md](../docs/ARQUITETURA.md) — o mapa das três peças
- [../docs/REDE.md](../docs/REDE.md), [../docs/UDP.md](../docs/UDP.md) — portas e firewall
