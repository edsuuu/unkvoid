# Como o Unkvoid conecta

O Unkvoid separa autoridade, sinalização e mídia em três peças. Essa divisão evita que o servidor que carrega contas e mensagens seja também o caminho que processa cada quadro de vídeo.

| Peça | Responsabilidade |
| --- | --- |
| Laravel | Contas, servidores, cargos, canais, mensagens e autorização. |
| SFU | Salas de voz, peers, produtores e consumidores de mídia. |
| Aplicativo desktop | Interface, captura, encoder e reprodução local. |

## Entrada em voz

O aplicativo pede ao Laravel um token para o canal de voz. O Laravel confere `CONNECT`, o limite de pessoas e as demais permissões; então assina um token HMAC de 60 segundos com o identificador do canal, a conta e as capacidades permitidas. O SFU verifica a assinatura e o vencimento, mas não decide a permissão.

O token é renovado antes de cada entrada, inclusive em uma reconexão. Uma conta mantém apenas uma sessão no SFU: uma nova entrada substitui a anterior.

## Caminho da mídia

A mídia não passa pelo Laravel. Para compartilhar a tela, o aplicativo captura o quadro como textura da placa de vídeo e usa o encoder de hardware para gerar o vídeo. Em seguida, envia RTP cifrado diretamente do aplicativo ao SFU por UDP; o SFU o encaminha aos consumidores sem decodificar o vídeo.

Esse caminho também leva o áudio da tela, microfone e, quando o cliente produz a fonte, câmera. O tráfego de mídia usa SRTP, com uma chave sorteada por transmissão. O receptor devolve pedidos de retransmissão e de quadro-chave por SRTCP quando necessário.

## Chat e presença

Chat, presença e atualizações em tempo real passam pelo WebSocket do SFU, mas a permissão de se inscrever em cada canal continua sendo consultada no Laravel. Uma pessoa sem `VIEW_CHANNEL` não recebe a assinatura nem o conteúdo do canal.

O Laravel e o SFU usam chamadas HTTP assinadas para ações administrativas, como mutar, mover ou remover alguém da voz. Essas chamadas são internas; não são uma API de integração pública.
