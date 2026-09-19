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

## A taxa do vídeo acompanha a perda, e não o REMB

**Decidido em 19/09/2026: o app baixa a taxa do encoder pelos pedidos de reenvio (NACK) que já
recebe do SFU, e não por estimativa de banda.**

A taxa era fixa por qualidade (10 Mb/s em 1080p60). Com o uplink saturado o app só contava
pacote largado e continuava empurrando a mesma taxa. A ideia de ajustar veio da análise de um
cliente alternativo de Discord (Litecord), que baixa a taxa quando quem assiste reporta perda.

O caminho "de livro" seria o REMB: o `plain.rs` já declara `goog-remb`, e o mediasoup 3.26 liga a
estimativa de banda quando o producer também traz a extensão `abs-send-time` (conferido em
`worker/src/RTC/Transport.cpp`). Não foi por aí, por três motivos medidos ou lidos no código:

- a estimativa é por **atraso entre pacotes**, e o app manda o quadro inteiro numa rajada, sem
  pacer. Um quadro-chave de 300 KB leva mais de 100 ms para escoar num uplink comum, e o
  estimador lê isso como fila crescendo: acusaria congestionamento a cada quadro-chave;
- o REMB nunca passa de 1,5× a taxa que está chegando. Tela parada manda 200 kb/s, o REMB diria
  300 kb/s, e seguir esse número estrangularia o encoder bem na hora em que a cena volta a mexer;
- fazer direito exige um pacer numa thread de envio — mexer no caminho que o app inteiro existe
  para proteger.

A perda não tem esses problemas: o NACK diz exatamente o que não chegou, e rajada só vira perda
quando a fila de alguém estoura — caso em que baixar a taxa **é** a resposta certa.

As regras (`media::BitrateGovernor`), e o porquê de cada número:

| Regra | Por quê |
|---|---|
| piso de 35% da taxa da qualidade | limita o estrago se a heurística errar: nunca cai para uma taxa que destrói a imagem |
| perda ≥ 5% numa janela de ~1 s → alvo × 0,70 | perda leve e aleatória (Wi‑Fi, a rota Brasil→EUA) não é congestionamento; o reenvio por NACK já cobre |
| depois de cada queda, 8 janelas sem poder cair de novo | o MFT da NVIDIA leva de 6 a 8 s para chegar à taxa nova (medido numa RTX 4060 Ti); decidir antes é punir duas vezes a mesma perda |
| perda < 1% por 5 janelas → alvo × 1,05 | sobe devagar, e a carência não conta como janela limpa |
| janela com menos de 100 pacotes não decide | tela parada não diz nada sobre a rede |
| `UNKVOID_ABR=off` | o mundo físico precisa de um botão de calibração |

O que mudaria a decisão: um pacer no remetente. Com ele o REMB passa a medir a rede, e não a
rajada, e vale trocar.

## Na retomada da sessão vale o `can` do token novo

**Decidido em 19/09/2026.** Era a pergunta aberta nº 5 do ESTADO: a retomada (os 30 s de
carência) ignorava o `can` do token que chega na reconexão. A resposta sai do princípio do
projeto — o Laravel decide, o SFU só confere a assinatura: quem foi mutado ou perdeu `STREAM`
durante a queda não pode voltar com a permissão antiga. O SFU fecha o producer que o `can` novo
não cobre. Só a tela revogada avisa o dono (`producerDead` com `reason: 'revoked'`): o app já
instalado derruba a transmissão com qualquer `producerDead`, e um aviso de microfone revogado
derrubaria uma tela que continua permitida. Token de outra conta não retoma a sessão, senão uma
conta herdaria o `can` de outra pela `resumeKey`.

## Imagem no chat: 3 por mensagem, 2 MB cada, reduzida no app

**Decidido em 19/09/2026.** O teto não é gosto: o PHP da VPS aceita 2 MB por arquivo e 8 MB por
pedido (`upload_max_filesize`, `post_max_size`), e mexer no `php.ini` da produção por causa de
print de jogo não se paga. Quem resolve é o app, que reduz a imagem antes de enviar (lado maior
de até 2560 px, WebP ou JPEG em degraus de qualidade até caber) — o que também tira o EXIF da
foto. GIF maior que 2 MB é recusado, porque reexportar mata a animação. Vai sob `SEND_MESSAGES`:
um bit novo de permissão só para anexo é regra de negócio que ninguém pediu. Mensagem direta
ainda não leva imagem: não há pivô para ela, e criar é migration.
