# NIMBUS — o servidor e a prova ponta a ponta (09/10/2026)

O que foi pedido: provar que transmitir (tela, câmera, microfone) e assistir não têm defeito no
caminho do servidor, com um harness que roda num comando e fala o mesmo protocolo do app nativo.
Branch `claude/sfu-e2e-harness-hvyijg`, sobre a `mac-dmg-sem-assinatura` de 09/10 (b487556).

## O que o harness cobre e como rodar

```bash
cd sfu && pnpm install
pnpm run e2e                      # todos os cenários, ~8 min, um SFU próprio por cenário
pnpm run e2e a c                  # só as letras pedidas
E2E_PHP=php8.4 pnpm run e2e e     # o "mover" sobe o web/ num sqlite descartável (PHP 8.4 e web/vendor)
```

Precisa de `ffmpeg`/`ffprobe`. Sem PHP 8.4, o cenário `e` é pulado e diz por quê. Os números ficam
em `sfu/e2e/out/report.json` e o `run.mjs` resume no fim. Detalhe de cada peça no
`sfu/README.md` ("Prova ponta a ponta").

O cliente sem tela (`sfu/e2e/lib/Participant.mjs`) reproduz o núcleo nativo: `join` com
`resumeKey` e identidade a cada entrada, `ping` de 5 s com 10 s de paciência, volta com `resume`
e, se a sala não o conhece mais, republica com chave e SSRC novos (`room.rs`); `producePlain` com
o `rtp_parameters` do `plain.rs` campo a campo (PT 96/111, SSRC base + origem, `nack`, `pli`,
`fir`, `goog-remb`), SRTP `AES_CM_128_HMAC_SHA1_80` escrito à mão, o `H264Payloader` (STAP-A,
FU-A, MTU 1200), SR de segundo em segundo, ritmo de 2,5× a taxa, reenvio do pacote cifrado no
NACK, quadro-chave no PLI/FIR com o freio do app; do outro lado, um socket só com furo de 5 s,
rota por SSRC, RTX desembrulhado e a recuperação do `recovery.rs` linha a linha. A mídia é H.264
de verdade (IDR do ffmpeg + quadros `P_Skip` montados à mão, com contador e hora de envio num SEI)
e Opus de verdade; o ffmpeg decodifica o que chegou. A rede ruim é um proxy UDP (perda, atraso,
jitter que não embaralha a fila, 1% embaralhado à parte) e o cabo puxado é ele junto com um proxy
TCP que segura o WebSocket meio aberto — o contêiner não tem `tc`.

| Cenário | O que prova |
|---|---|
| a | tela 1080p60 + câmera 360p30 + microfone para duas pessoas: em ordem, sem quadro pulado, sem parada > 500 ms, no tamanho e no fps pedidos; o ffmpeg decodifica sem erro |
| b | entrar atrasado: 1º quadro em até 1 s só pelo PLI (GOP desligado), um por vez, dois a 300 ms, três juntos; e um quadro-chave por atrasado |
| c | 5% de perda + jitter na subida e na descida: recupera por NACK/RTX/PLI sem parar > 1 s; nos dois lados (≈10% de ponta a ponta), sem parar > 1,5 s |
| d | trocar resolução, trocar de tela, parar tudo e recomeçar, duas telas, oito assistindo, pausar e retomar (quem assiste e quem transmite) |
| e | `PATCH /api/channels/{origem}/voice/members/{user}` do Laravel real no meio da transmissão |
| f | worker morto (só a sala dele cai, 1012, volta sozinha); sala em dois workers por `pipeToRouter`; sala que encolhe |
| g | rede cortada 10 s (quem assiste e quem transmite); 200 ciclos de entrar/transmitir/assistir/sair, 1 em 4 sem `leave` |
| h | 10 pessoas, 2 telas 1080p30, 10 câmeras 360p30, todo mundo assistindo tudo: CPU do SFU |

A rota nova `GET /stats` (assinada, `docs/CONTRATO.md`) conta o que o mediasoup segura de verdade
(os `dump()` de cada router, pipes incluídos), a memória e a CPU de cada worker. É por ela que o
harness acha vazamento e mede a CPU.

## Os números (rodada final, contêiner de 4 núcleos Xeon 2,8 GHz)

| Medida | Resultado |
|---|---|
| a. tela 1080p60 / câmera 360p30 | 60 fps e 30,1 fps, 0 quadro pulado, 0 fora de ordem, pior parada 34 ms, atraso p99 ~17 ms |
| b. 1º quadro de quem entra atrasado | 23–70 ms um por vez e em grupo; ~0,45 s no pior caso, quem entra logo depois do quadro-chave de outro (o freio de 0,5 s do `keyFrameRequestDelay`; com o freio antigo de 1 s, 0,94–1,07 s) |
| b. custo para quem transmite | 1 quadro-chave por vídeo por atrasado (antes da correção: 2) |
| c. 5% na subida (cliente com RR e DLRR) | pior parada 153–827 ms em 21 de 22 rodadas, uma de 1,3 s; 0–3 buracos em ~11 mil pacotes |
| c. 5% na descida | 154–985 ms em 7 rodadas; 0–7 buracos |
| c. 5% nos dois lados (≈10% ponta a ponta) | 360–1300 ms em 8 rodadas (teto do teste: 1,5 s) |
| c. o app de hoje, 5% na descida | 3,6–7,2 s parado, 69–183 de 450 quadros (ver bugs do app, 1 e 2) |
| d. tela trocada / recomeçada / retomada | 1º quadro em 32 ms / 35 ms / 85 ms |
| e. mover | origem para de receber em 38–56 ms, destino vê a tela 138–156 ms depois do PATCH, 0 pacote fantasma, origem responde 403 ao token, `left`/`joined` no tempo real |
| f. worker morto | quem assistia volta a ver em 0,94–1,02 s (inclui a espera sorteada de 0,5–1 s do app antes de reconectar); a outra sala: 0 queda, sem parada > 500 ms |
| g. rede cortada 10 s | a imagem volta 80–98 ms depois da rede; mesmo número de transports/producers/consumers antes e depois |
| g. 200 ciclos | estado final idêntico ao de base; memória +12 MB (≈10%) do ciclo 50 ao 200, heap do Node estável (18–23 MB) |
| g. 600 ciclos (à parte) | estado final idêntico; workers +2,4 → +2,0 → +0,9 MB a cada 150 ciclos (desacelerando) |
| h. carga, um router (como na VPS) | 198 consumers, ~33 mil pacotes/s saindo: **56–58% de um núcleo** no worker da sala, Node 0,4%; pior parada 105 ms, 0 áudio perdido; memória do SFU ~260 MB |
| h. a mesma carga em dois workers | 30–32% em cada (61–65% somado: o pipe custa ~5 pontos); pior parada 83 ms |

Leitura da carga: uma sala de 10 com duas telas e todas as câmeras usa pouco mais de meio núcleo.
Na VPS (4 vCPU, 3 workers) cabem duas ou três salas assim antes de um núcleo encher; o
`SFU_PEERS_PER_ROUTER=10` faz a sala inteira cair num worker só.

## Bugs do SFU achados e corrigidos

1. **O `resumeConsumer` pedia quadro-chave duas vezes** —
   `sfu/src/Http/Controller/ConsumerController.ts:116`. O `consumer.resume()` do mediasoup já
   pede o quadro-chave; o `requestKeyFrame()` explícito logo depois caía no freio de 1 s do
   `keyFrameRequestDelay`, saía um segundo depois e rearmava o freio. Cada pessoa que abria uma
   tela custava dois quadros-chave a quem transmite (o quadro mais caro, o que trava upload
   fraco), e quem entrava 1–2 s depois de outro esperava até 1 s a mais. Para o app nativo é pior:
   o segundo pedido cai no espaço de 2 s do `KeyframeGate` e dobra o espaço dele para 4 s.
   Teste: `b. cada atrasado custa um quadro-chave por vídeo a quem transmite, não dois` — antes,
   `tela 2, câmera 2` em cada atrasado e 865 ms para quem entrou 1,1 s depois de outro; depois,
   1/1 e 43–77 ms. O `d. quem assiste pausa e retoma` prova que a retomada continua pedindo um.
2. **A sala que encolhia não devolvia o router do outro worker** — `sfu/src/Services/Room.ts:446`
   (`shrink`, chamado no `removePeer`). Quando a sala passava de 10 pessoas ia para um segundo
   worker, e quando todo mundo de lá saía o router e o par de pipes ficavam até a sala acabar: o
   worker de origem continuava mandando cada pacote de cada tela para um router vazio, noutro
   núcleo. Agora o router sem ninguém fecha (fica sempre um; nada fecha com alguém escolhendo
   router ou com a sala abrindo outro), e o mediasoup fecha o par de pipes junto. Testes:
   `f. a sala que encolhe devolve o router do outro worker` (antes: 2 routers, +2 transports,
   +2 producers e +2 consumers sobrando) e `g. 200 ciclos` (antes: o mesmo resto depois que os
   órfãos da carência passaram de 10).
3. **O freio de quadro-chave de 1 s tornava o "até 1 s" impossível** —
   `sfu/src/Http/Controller/ProducerController.ts:13` (`KEYFRAME_REQUEST_DELAY_MS`, o
   `keyFrameRequestDelay` do mediasoup), agora 500 ms. Pedido que chega dentro do freio espera ele
   acabar: quem entrava logo depois do quadro-chave de outra pessoa esperava ~1 s mais a viagem do
   quadro. Visto primeiro no grupo de três que entra junto (o terceiro com 1069 ms). Teste:
   `b. quem entra logo depois do quadro-chave de outro (dentro do freio do mediasoup)`, com 15 ms
   por sentido — antes 936–1021 ms (falha), depois 429–502 ms. O app continua protegendo o
   encoder com o freio de 2 s dele; o do SFU só junta os pedidos da sala.
4. **`GET /stats` novo** — `sfu/src/Services/RoomRegistry.ts` (`dump`). Não é correção de
   defeito, é o instrumento: sem ele não há como ver vazamento no mediasoup (o objeto que o SFU
   esqueceu e o worker segura). Aguenta router fechando e worker morrendo no meio da conta.

O `check.mjs` (39) e o `check-realtime.mjs` (11) passam com as mudanças; `eslint` e `tsc` limpos.
O Laravel do cenário `e` roda com um worker do PHP só: com sqlite, dois escrevendo `channel_accesses`
ao mesmo tempo (o webhook do SFU e o token de voz) deram 500 uma vez. É coisa do sqlite do teste;
a VPS usa MySQL.

## Bugs do app nativo a repassar (não corrigidos aqui)

Cada um tem um teste `todo` que reproduz (aparece como `# TODO` no relatório do `node --test`) ou
uma chave do cliente do harness que liga e desliga o comportamento.

1. **O receptor não manda RTCP RR** (`native/shared/media/src/receiver.rs`). Sem RR o mediasoup
   não sabe a ida e volta até quem assiste e fica com 100 ms fixos; ele não reenvia o mesmo
   pacote duas vezes dentro desse tempo. O `recovery.rs` pede três vezes a cada 40 ms
   (`RETRY_FLOOR`, `MOST_ASKS`): se o primeiro reenvio se perde, os outros dois pedidos são
   ignorados, o buraco é largado em 250 ms e vira PLI. Com 5% de perda na descida: 11–25 buracos
   em 15 s contra 0–7 com RR. Cenário: `c. o app de hoje com 5% na descida` (3,6–7,2 s parado, 69–183
   de 450 quadros); `receiverReports: true` no harness mostra a correção (RR a cada 1 s com
   LSR/DLSR do último SR de cada fluxo).
2. **O remetente não responde o XR RRTR com DLRR** (`native/shared/media/src/plain.rs`). O
   mediasoup manda RRTR a quem transmite e mede a ida e volta pelo DLRR; sem ele repete o NACK da
   subida a cada 100 ms fixos, e a perda dupla na subida passa dos 250 ms do receptor. Com 5% na
   subida: 2–6 buracos e até 1,35 s parado, contra 0–3 buracos e quase sempre menos de 0,9 s com DLRR
   (`extendedReports: true` no harness: bloco 5 com o SSRC da própria origem, que é como o
   mediasoup procura o producer).
3. **Freio de quadro-chave de 2 s** (`native/shared/core/src/sharing.rs:39`, `KEYFRAME_SPACING`,
   dobrando até 4 s). Quem entra até 2 s depois de outro espera o freio: 1,8–2,3 s para o
   primeiro quadro. Com perda, cada buraco espera o mesmo freio. Cenário: `b. com o freio de
   quadro-chave do app (2 s)`. O SFU freia em 0,5 s (`keyFrameRequestDelay`); os dois somam.
4. **O pedido de quadro-chave não separa tela de câmera** (`plain.rs:495`, `read_feedback`, e
   `wants_keyframe` sem olhar SSRC). Tela e câmera dividem um `pending` só: a primeira origem que
   lê leva o pedido, e a outra espera o GOP de 4 s. Cenário: `b. com o pedido de quadro-chave
   dividido` — câmera do atrasado em 0,7–2,8 s; `keyframeRouting: 'shared'` no harness.
5. **A câmera derruba o ritmo da tela** (`sharing.rs:979`, `follow_bitrate` a cada quadro com a
   taxa da própria origem). O ritmo (`pacer.rs`) é um só por remetente; com tela e câmera, quem
   mandou o último quadro decide, e a câmera põe o piso de 4 Mb/s. A tela de 6 Mb/s fila: atraso
   p99 de 302 ms contra ~20 ms com o ritmo somado, e picos de 300–400 ms no primeiro quadro de
   quem entra. Cenário: `a. tela e câmera juntas no ritmo do app`; `pacing: 'native'` no harness.
6. **O histórico de reenvio não olha o SSRC** (`plain.rs:501`). O NACK da câmera procura o número
   de sequência num histórico que mistura tela e câmera; números iguais nos dois fluxos reenviam
   o pacote errado (o servidor descarta como repetido e o buraco fica). Raro (sequências
   sorteadas), sem cenário; o harness guarda por SSRC.

Já resolvido na branch de integração (conferido no código de 09/10): o `moved` no `session.rs` e no
`room.rs`. O cenário `e. o app antigo, que não conhece moved` continua provando a trava do servidor
para o app antigo: ele volta para a origem, o Laravel recusa (403) e ele não reaparece lá.

## Observações (sem defeito provado)

- **Perda no pipe entre workers.** Numa rodada de carga com o harness saturando a CPU (um processo
  só), só quem estava no outro worker perdeu os mesmos 4–17 pacotes da tela, e o
  `pipeToRouter` não tem RTX (padrão do mediasoup), então essa perda não volta. Com o harness em
  threads (como ficou) não se repetiu em oito rodadas, com e sem RTX. Se aparecer na VPS sob CPU
  alta, `enableRtx: true` no `Room.pipe` é a saída.
- **Portas de RTP puro.** 64 por worker, duas por pessoa (subida e chegada), e quem cai segura as
  duas pelos 30 s de carência. 600 entradas em 2,5 min com 1 em 4 caindo esgotaram as portas
  (`o servidor já está no limite`), como o `docs/UDP.md` prevê. Não é vazamento; é o teto.
- **O `c` encosta em 1 s.** Com 5% de perda aleatória, o raro buraco que sobra espera o prazo do
  receptor (o `recovery.rs` alonga o prazo até 1 s quando o reenvio demora) e o PLI. Em 22
  rodadas da subida, uma passou (1,3 s, ainda com o freio de 1 s); nos dois lados, uma de 8 deu
  1,3 s, por isso o teto ali é 1,5 s. Se o `pnpm run e2e` falhar no `c`, rode `pnpm run e2e c`
  de novo antes de concluir que algo mudou.

## O que só máquina real prova

- O encoder de hardware (NVENC, QSV, AMF, VideoToolbox) atendendo o PLI no quadro seguinte, o
  tamanho real do quadro-chave e o decodificador de verdade de quem assiste; aqui o vídeo é
  sintético e o quadro P não carrega imagem.
- A NAT de casa, Wi-Fi, 4G e a troca de endereço no meio (o `comedia` preso ao endereço antigo);
  aqui todo mundo está em 127.0.0.1 e o endereço não muda.
- A perda e o jitter de verdade (em rajada, com fila de roteador), o RTT São Paulo ↔ Brasil e o
  upload fraco; o proxy daqui é perda aleatória e atraso de 10–30 ms.
- A CPU da VPS dividida com Laravel, nginx e MySQL, e o firewall das portas UDP (`docs/UDP.md`);
  os números de CPU acima são de um contêiner com 4 núcleos só para o teste.
- O jogo aberto ao mesmo tempo, que é a regra do projeto: nenhum número aqui mede o fps do jogo.
