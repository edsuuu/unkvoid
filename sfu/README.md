# SFU próprio

Nosso servidor de mídia. Roda em Node sob pm2 na VPS.

**Nós escrevemos:** protocolo de sinalização, modelo de sala/participante, roteamento,
política de codec e camadas, ciclo de vida de producer/consumer.
**O mediasoup faz:** ICE, DTLS, SRTP, RTP/RTCP, NACK, estimativa de banda e seleção de
camada — o mesmo papel que o Pion faz dentro do LiveKit.

```
client/SfuClient.js   protocolo + captura + publicação
client/App.js         interface
src/SignalingServer.js  WebSocket, despacho de ações, WebRtcServer
src/Room.js           router do mediasoup, peers, produce/consume
src/Peer.js           transports, producers, consumers de um participante
src/config.js         codecs, bitrates, porta de mídia
```

## Protocolo

Pedido/resposta com `id`, mais eventos empurrados pelo servidor.

```
→ { id, action, data }
← { id, ok: true, data } | { id, ok: false, error }
← { event, data }
```

Ações: `join`, `createTransport`, `connectTransport`, `produce`, `closeProducer`,
`consume`, `resumeConsumer`, `setPreferredLayers`, `consumerStats`.
Eventos: `peerJoined`, `peerLeft`, `newProducer`, `producerClosed`, `consumerClosed`.

## ⚠️ O mediasoup é ICE Lite — isso decide a config de firewall

O mediasoup **só responde** a checagens ICE; nunca manda a primeira. Atrás de um
firewall stateful, isso significa que a porta de mídia **precisa estar liberada para
entrada não solicitada**. O LiveKit (Pion) faz ICE completo e manda o primeiro pacote
pra fora, abrindo o buraco sozinho — por isso ele funcionou nesta VPS sem liberar nada.

Medido, não suposto: com o LiveKit parado, um listener UDP na 7882 **não recebeu**
pacote nenhum de fora. O mesmo na 40050. O filtro da Contabo derruba UDP não solicitado.

Por isso o servidor usa **`WebRtcServer`**, que multiplexa todos os transports em
**uma única porta** (40000) em vez de uma porta por transport. O pedido de firewall
vira uma linha em vez de uma faixa de 200.

### Liberar na Contabo

| Porta | Protocolo | Para quê |
|---|---|---|
| 40000 | **UDP** | mídia — sem isso nada conecta |
| 40000 | TCP | fallback para redes que bloqueiam UDP |

Já liberadas hoje: 22, 80, 443, 8443, 30033/tcp e 9987/udp.

## Deploy

```bash
./deploy.sh
```

O `pnpm install` **não** baixa o worker do mediasoup sozinho (pnpm 11 ignora
`onlyBuiltDependencies` do package.json), então o deploy roda o postinstall na mão.
O binário é pré-compilado — a VPS não precisa de compilador.

## Local

```bash
pnpm install && pnpm run build:client
SFU_ANNOUNCED_ADDRESS=127.0.0.1 node src/server.js
cd public && python3 -m http.server 8080
```

Abre `http://localhost:8080/?sfu=ws://localhost:3000/sfu`.
