# Decisões de arquitetura

Registro do que foi decidido, e principalmente do **porquê**. Decisão sem motivo
escrito vira discussão de novo daqui a seis meses.

## Em que linguagem o SFU deve ser escrito

**Decidido em 09/09/2026: fica como está, Node com mediasoup.**

A pergunta apareceu como "dá para fazer em PHP?", depois "e em Java ou Rust?".
Nas três a resposta técnica é diferente da resposta prática.

### Primeiro, o que é o SFU aqui

Duas coisas moram no mesmo processo, e elas têm custos muito diferentes:

| Camada | O que faz | Onde roda hoje |
|---|---|---|
| Sinalização | entrar na sala, publicar, consumir, sair | JavaScript, no processo Node |
| Mídia | mover pacote de vídeo e áudio | C++, nos workers do mediasoup |

O Node **não vê um único pacote de vídeo**. Ele só troca mensagens. Quem move
mídia é um processo C++ separado, um por núcleo. Isso está documentado no
`RoomRegistry.ts` e é o que faz a conta fechar em 1440p60.

Trocar "a linguagem do SFU" quase sempre quer dizer trocar a camada de mídia, que
é a cara. Trocar só a sinalização é barato e ganha pouco.

### PHP

Sinalização, sim. Mídia, não.

O plano de mídia é cifrar e decifrar SRTP pacote a pacote, montar RTP, negociar
ICE e DTLS e controlar congestionamento, na ordem de dezenas de milhares de
pacotes por segundo por sala. Não é questão de gosto: não existe SFU de produção
em PHP, e os que existem são C, C++, Go ou Java.

### Java

Tem prova de campo. O Jitsi Videobridge é um SFU em Java rodando em produção há
anos e em escala. Se o critério for "alguém já fez", esta é a resposta segura.

### Rust

É o encaixe mais natural, e não por gosto: **a pilha já está neste repositório**.
No `native/Cargo.lock`:

```
rtc-ice   rtc-dtls   rtc-srtp   rtc-rtp   rtc-rtcp
rtc-sctp  rtc-sdp    rtc-media  rtc-interceptor
```

Isso é WebRTC em Rust, e compila a cada build do app. O `crates/media` já
empacota RTP, cifra SRTP e monta payload de H.264.

### Por que, mesmo assim, não trocar

O que o mediasoup entrega de graça, e que teria de ser reescrito:

- ICE-lite e DTLS-SRTP
- estimativa de banda e controle de congestionamento
- retransmissão de pacote perdido (NACK) e pedido de keyframe (PLI/FIR)
- simulcast e SVC
- anos apanhando de rede real

O ganho seria uma linguagem em vez de duas e um processo a menos. Real, mas
pequeno perto de meses de trabalho cujo melhor resultado é **empatar** com o que
já funciona.

Tem uma assimetria que ajuda a ver o tamanho. A metade de **quem transmite** já
está resolvida em Rust aqui: o app escolhe o SSRC, a chave SRTP e o tipo de
payload, e manda RTP puro direto, sem ICE e sem DTLS, porque o servidor aceita
assim (`crates/media/src/plain.rs`). A metade cara é a de **quem assiste** — ICE,
DTLS e negociação com cada navegador, com toda variação de NAT e rede. É essa que
o mediasoup faz.

### O que mudaria a decisão

Não é "ficar bonito ter um só idioma". Seria um destes:

1. O mediasoup virar gargalo medido, não suposto.
2. Precisar de algo que ele não faz e não dá para contornar.
3. O processo Node virar fonte recorrente de incidente.

Nada disso aconteceu. O SFU acabou de ganhar heartbeat e faixa de portas maior, e
está em produção.

### O que **não** depende disto

Servidores com salas (ver [ESTADO.md](ESTADO.md)) é decisão de produto, não de
infraestrutura, e não depende da linguagem do SFU. Ela precisa de nome, convite,
membros, permissões e persistência — CRUD com banco de dados.

Aí PHP encaixa bem, e existe Laravel em casa. O arranjo natural é o Laravel
decidir quem pode entrar em qual sala, e o SFU continuar só com a mídia,
perguntando se o código é válido. Isso também responde onde guardar o estado dos
servidores, que hoje é a pergunta em aberto: o projeto não guarda nada, sala
existe enquanto tem gente dentro.

Se for para gastar energia numa das duas frentes, é nesta.
