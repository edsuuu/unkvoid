# Auditoria de transmitir e assistir — 09/10/2026

Revisão adversarial do caminho inteiro, do botão ao pixel, pedida pelo dono em 09/10/2026: "ele
precisa garantir e não ter um único bug de transmissão, assistir, ou algo relacionado".
Transmitir é publicar tela, câmera e áudio; assistir é receber os dos outros.

- **Base:** `origin/mac-dmg-sem-assinatura` em `df05677`, já com os merges de `feat/discord-linux`
  (#43), `fix/linux-tela`, `feat/discord-windows` (#45) e `feat/discord-macos` (#46). A auditoria
  começou em `5a33c18` e foi refeita sobre cada merge que entrou.
- **Método:** cada suspeita foi provada com teste ou roteiro reproduzível neste contêiner (Rust no
  Linux, o SFU de verdade em Node com mediasoup 3.26 e SRTP de verdade, o Laravel com Pest) antes
  de entrar aqui. O que não reproduziu está em [Descartados](#descartados), com a prova de que não
  é defeito. O Swift do macOS não compila aqui: o que é só dele foi provado por leitura do código,
  e está marcado assim.
- **Ninguém corrigiu nada:** este relatório e os testes de reprodução são a entrega. Os donos:
  **Nimbus** (`sfu/` e a voz do `web/`), **Stratus** (`native/shared` e `native/apps/windows`),
  **Mirror** (`native/apps/macos`).

| | Quantos |
|---|---|
| P0 — quebra ou trava transmitir/assistir | 5 |
| P1 — degrada | 10 |
| P2 — risco | 24 |
| Descartados com prova | 29 |

## Como rodar as provas

Cada teste de reprodução **afirma o comportamento certo e falha hoje**; os do Rust estão com
`#[ignore]` para o `cargo test` de todo dia continuar verde. Quando a correção entrar, o teste
passa: tire o `#[ignore]` e ele vira guarda. A exceção é o arquivo do Laravel, que afirma o
comportamento de hoje (veja a linha dele).

| Onde | Comando | O que sai hoje |
|---|---|---|
| `native/shared/media/tests/audit.rs` | `cd native && cargo test -p media --test audit -- --ignored` | 3 falham (P0-1, P2-1, P2-2) |
| `native/shared/core/tests/audit.rs` | `cd native && cargo test -p core-app --test audit -- --ignored` (os de SFU real pedem `UNKVOID_AUDIT_SFU`, abaixo) | 2 falham (P1-2, P2-4) |
| os dois acima, sem `--ignored` | `cargo test -p media --test audit && cargo test -p core-app --test audit` | passam (guardas e descartados) |
| contra o SFU de verdade (Rust) | `cd sfu && pnpm run build && SFU_SECRET=$(printf 'a%.0s' {1..40}) SFU_CONNECTIONS_PER_MINUTE=1000 node dist/server.js` e, noutro terminal, `cd native && UNKVOID_AUDIT_SFU=ws://127.0.0.1:3000/sfu cargo test -p core-app --test audit real_sfu -- --ignored --nocapture --test-threads=1` | passam (descartados D1 e D2) |
| `sfu/audit/*.mjs` | `cd sfu && pnpm run build`, depois `node audit/<arquivo>.mjs` (cada um sobe o próprio SFU em portas próprias) | `FAIL` = defeito reproduzido |
| `docs/auditoria/web/VoiceAuditReproTest.php` | `bash docs/auditoria/run-web.sh` (copia para `web/tests`, roda o Pest e apaga) | `OK (22 tests)`: aqui **passar** = os 16 `REPRO` se reproduzem e os 6 `DISCARD` estão certos |

Os roteiros do SFU (`lib.mjs`, `srtp.mjs`) falam o protocolo do app: `join`, `producePlain`,
`consumePlain`, RTP e RTCP cifrados em SRTP por UDP. Nada antes desta auditoria mandava mídia de
verdade contra o SFU (o `check.mjs` não manda pacote).

## P0 — quebra ou trava transmitir/assistir

### P0-1 · Depois de ensurdecer (ou ligar o som de uma tela) passados 11 min, o som some por minutos

- **Onde:** `native/shared/media/src/unpack.rs:143-150`, alimentado por
  `native/shared/media/src/receiver.rs:336-338` e `native/shared/core/src/watching.rs:189-191`,
  `:237-245`. Vale para o Windows, o Linux (Slint) e o macOS: os três assistem pelo
  `core::watching`.
- **Cenário:** ensurdecer (ou o som de uma tela, que chega mudo por regra) faz o `PlainReceiver`
  parar de repassar os pacotes daquela rota; o servidor continua numerando. Na volta, o
  `AudioUnpacker` compara o número novo com o último que viu **antes** do mudo. Se o mudo passou
  de 32 767 pacotes (10,9 min a 50 por segundo), a diferença cai na metade de cima dos 16 bits, o
  pacote é tomado por "atrasado" (`missing >= u16::MAX / 2`), volta `None` e o `last` não anda —
  e todos os seguintes também, até a conta dar a volta. Ensurdecido 15 min: 6,8 min sem ouvir
  ninguém. Ligar o som de uma tela que se assiste há 15 min: o mesmo. Mutar o microfone do outro
  lado **não** causa isso (o mediasoup ressincroniza a numeração ao retomar; ver D6).
- **Prova:** `cargo test -p media --test audit audio_comes_back -- --ignored`
  ```
  assertion `left == right` failed: o som só voltou depois de 20537 pacotes (411 s de silêncio) — o mudo durou 15 min
  ```
  Contraprova (passa): `audio_comes_back_after_a_short_local_mute` — um minuto de mudo volta na hora.
- **Correção:** o mudo não pode apagar o estado da numeração. A menor: ao desmutar a rota
  (`PlainReceiver::set_muted(false)`), zerar o `last` do `AudioUnpacker` daquele producer (um aviso
  pela própria rota, ou repassar o pacote ao `pump` com a marca de mudo e calar depois de
  decodificar). Alternativa: tratar diferença maior que alguns segundos de pacotes como fluxo
  novo, como o `Recovery` faz com `JUMP`, guardando a hora do último pacote.
- **Dono:** Stratus.

### P0-2 · Quem é movido para um canal trancado ou cheio cai da voz na primeira oscilação de rede

- **Onde:** `web/app/Models/Channel.php:360` (o passe do mover é `Cache::pull`: vale **uma**
  entrada), `:362-364` e `:368` (o `CONNECT` e o `user_limit` voltam a ser conferidos); do lado do app,
  `native/shared/core/src/session.rs:236` e `native/shared/core/src/ffi.rs:155` /
  `native/apps/windows/src/bridge.rs:2961`, que pedem token novo antes de **cada** `join`,
  inclusive na retomada.
- **Cenário:** um moderador move alguém para um canal sem `CONNECT` para `@everyone` (um "palco",
  um AFK trancado) ou cheio. A primeira entrada usa o passe. Qualquer queda de sinalização depois
  (o ping de 10 s, uma troca de Wi-Fi) leva o app a pedir token para a retomada: 403. A retomada
  falha, a entrada nova também, oito recusas e o app desiste (`SESSION_GONE`): a pessoa sai da voz
  e não volta. O contrato se contradiz: "uma vez, por 60 s" no passe e "o app pede um token novo
  antes de cada `join`, inclusive nas reconexões".
- **Prova:** `bash docs/auditoria/run-web.sh` — `REPRO M2` (canal sem `CONNECT`: o token da
  reconexão é recusado) e `REPRO M2b` (canal cheio: `"O canal está cheio."` na reconexão).
- **Correção:** quem já está sentado no canal (acesso aberto em `channel_accesses` pelo webhook
  `joined`, fechado no `left`) não passa de novo por `CONNECT` nem pelo limite; e o passe usa
  `Cache::get` e vale os 60 s inteiros. Mudar o contrato na mesma tarefa.
- **Dono:** Nimbus.

### P0-3 · Mover alguém de volta em menos de 60 s tira a pessoa da voz

- **Onde:** `web/app/Models/Channel.php:356` confere a marca de "acabou de sair daqui" **antes**
  do passe (`:360`); o mover de A para B grava essa marca para A (`:418`).
- **Cenário:** o moderador move para o canal errado e devolve (A→B→A) dentro de 60 s. O app do
  movido recebe `moved { to: A }`, pede o token de A e leva 403 "Você acabou de ser movido para
  outro canal.": fica fora da voz.
- **Prova:** `bash docs/auditoria/run-web.sh` — `REPRO M1`.
- **Correção:** no `move()`, apagar a marca de saída do **destino**
  (`Cache::forget($destination->moveOutKey($target))`), ou conferir o passe antes da marca.
- **Dono:** Nimbus.

### P0-4 · Quem entrou mutado pelo servidor não volta a falar quando o moderador desmuta

- **Onde:** `web/app/Models/Channel.php:372` tira `speak` do token enquanto `server_mute`;
  `web/app/Models/Server.php:488-493` só chama `/mute false`; `sfu/src/Services/Peer.ts:120-130`
  confere o `can` do `join`, que não muda.
- **Cenário:** alguém entra (ou reconecta) mutado pelo servidor. O moderador desmuta: o SFU tira a
  marca de mudo, mas o `can` da sessão continua sem `speak` — o `producePlain` do microfone é
  recusado (403) e o app esconde o microfone (`canSpeak: false`). Só saindo e entrando de novo.
- **Prova:** `bash docs/auditoria/run-web.sh` — `REPRO S2` (o token sai sem `speak` e o desmutar
  só manda `/mute {muted:false}`); o lado do SFU, por leitura de `Peer.ts:120-130`
  (`allows()` lê `this.can`, gravado no `join`).
- **Correção (muda o contrato: o SFU sobe antes):** `speak` sai só da permissão `SPEAK`, e o mudo
  do servidor vale pela marca do SFU — por exemplo, o webhook `joined` chama `/mute true` quando
  `server_mute` está ligado.
- **Dono:** Nimbus.

### P0-5 · macOS: com AirPods (ou troca de saída de som), ninguém é ouvido até outra pessoa entrar — provado por leitura

- **Onde:** `native/apps/macos/Sources/Unkvoid/Platform/Sound.swift:238-262` (`player(for:)`) e
  `:55` (`scheduleBuffer`); nenhum observador de `AVAudioEngineConfigurationChange` no app.
- **Cenário:** o `AVAudioEngine` de saída para sozinho quando a taxa ou os canais do dispositivo
  mudam — o que acontece ao abrir o microfone dos AirPods (eles passam ao modo de chamada) e ao
  trocar de saída no meio da chamada. O `joinVoice` toca o som de entrada e já cria os tocadores
  das vozes antes de `openMicrophone()` (`AppModel+Room.swift:61-66`). Depois da parada, os
  tocadores que já existem não recebem `play()` de novo (`player(for:)` só chama `play()` no
  tocador novo, `:259`) e seguem empilhando blocos num motor parado: silêncio até alguém novo
  começar a mandar som.
- **Prova:** leitura (o Swift não compila neste contêiner; o gatilho precisa de um Mac com fone
  Bluetooth). Conferido na base `df05677`: `grep -rn ConfigurationChange native/apps/macos` não
  acha nada.
- **Correção:** observar `AVAudioEngineConfigurationChange` do motor de saída e, nele, religar o
  motor e chamar `play()` em cada tocador (e reaplicar a saída escolhida).
- **Dono:** Mirror.

## P1 — degrada

### P1-1 · Quem transmite parar de mandar faz todo espectador refazer o caminho de chegada inteiro

- **Onde:** `sfu/src/Http/Controller/ProducerController.ts:97-113` (o `producerReceiving` sai da
  nota do mediasoup) — no mediasoup 3.26 a nota só zera por inatividade em simulcast com mais de
  um fluxo (`worker/src/RTC/Producer.cpp`, `useRtpInactivityCheck`), e o app manda um fluxo só.
  Do lado do app, `native/shared/core/src/room.rs:136-166` e
  `native/shared/core/src/watching.rs:389-424`.
- **Cenário:** a subida de quem transmite morre (Wi-Fi, NAT que trocou de porta, captura travada
  por mais de 5 s). O SFU continua dizendo `receiving: true` para sempre; cada espectador vê 5 s
  sem pacote e conclui que o caminho **dele** morreu: troca a chave, derruba o transporte de
  chegada e reassiste tudo — as outras telas, as câmeras e o microfone de todo mundo —, e repete a
  10, 20, 40 e 60 s enquanto durar. A sala inteira engasga por causa de uma pessoa. O contrato diz
  "o mediasoup zera a nota ~1,5 s depois do último pacote": não é verdade nesta configuração.
- **Prova:** `node sfu/audit/receiving-flag.mjs`
  ```
  FAIL  (a) producerReceiving=false after the publisher stops sending (contract: ~1.5 s)  — never emitted in 15 s
  FAIL  (a) a fresh consumePlain 15 s after the publisher went silent says receiving=false  — receiving=true
  FAIL  (b) ...but the SFU says receiving=true and never flips it ...  — consumePlain.receiving=true, receiving=false events=0
  ```
  A reação do app a `receiving: true` sem pacote é a do teste existente
  `the_arrival_path_is_rebuilt_only_when_the_server_receives_and_nothing_comes`.
- **Correção:** no SFU, um relógio próprio (contador de pacotes do `getStats()` a cada 1–2 s) no
  lugar da nota, e `receiving && !producer.paused`. Atualizar o contrato.
- **Dono:** Nimbus.

### P1-2 · Duas chamadas de `consume_all` ao mesmo tempo abrem dois consumers do mesmo producer; o que chega nunca fecha

- **Onde:** `native/shared/core/src/room.rs:380-426` e `:430-503` (o `is_watching` é conferido
  antes do `await` do `consumePlain` e marcado só depois); `native/shared/core/src/watching.rs:151`.
- **Cenário:** o `settle` da entrada, o `newProducer`, o "Assistir" (`watch`) e o vigia do caminho
  de chegada chamam `consume_all` por tarefas diferentes. Duas ao mesmo tempo pedem dois
  `consumePlain` do mesmo producer; o segundo `Watching::start` não faz nada, mas o id dele
  sobrescreve o do primeiro em `consumers`. O consumer que de fato chega fica órfão: pausar (janela
  fora da vista), fechar e "Assistir" agem no outro, e a banda dele não para nunca. Com chave nova
  (logo depois de um `rewatch`) soma-se ao P1-3 e o segundo transporte derruba tudo o que se
  assistia (`watching.rs:143-149`).
- **Prova:** `cargo test -p core-app --test audit one_producer -- --ignored`
  ```
  consumer retomado e nunca fechado: ["consumer-0"] (consumePlain por producer: {"tela-ana": 2}, retomados ["consumer-0", "consumer-1"], fechados ["consumer-1"])
  ```
- **Correção:** um conjunto "assistindo ou abrindo" marcado **antes** do `await` (e limpo na
  falha), ou um `tokio::sync::Mutex` em volta de `consume_all`.
- **Dono:** Stratus.

### P1-3 · `producePlain`/`consumePlain` simultâneos de uma pessoa abrem dois transportes; um guest esgota as portas do worker

- **Onde:** `sfu/src/Services/Room.ts:497-542` (`plainTransport`: confere o `existing` antes do
  `await this.routerOf` e cria outro).
- **Cenário:** microfone e tela (ou câmera) abertos no mesmo instante: duas portas; o app manda
  tudo para a da última resposta (`sharing.rs:491-510`) e o outro producer não recebe nada — o SFU
  o derruba em 30 s e o app avisa "o servidor fechou o microfone". Do lado de assistir, ver P1-2.
  Disparando `consumePlain` com chaves diferentes ao mesmo tempo, um único socket sem conta prende
  todas as portas de RTP puro do worker até sair, e ninguém mais publica nele.
- **Prova:** `node sfu/audit/concurrent-plain.mjs` e `node sfu/audit/port-exhaustion.mjs`
  ```
  FAIL  concurrent producePlain with the same key reuse ONE send transport  — ports 23414 vs 23416
  FAIL  both producers receive media when sent to the last port  — mic receiving=false screen receiving=true
  FAIL  concurrent consumePlain with the same key share ONE receive transport  — ports 23408 vs 23411
  FAIL  concurrent rekeys leave at most one receive transport  — 6 distinct ports still allocated
  FAIL  a different room can still publish its screen  — 422 o servidor já está no limite de participantes por sala
  ```
  No app Slint de hoje o microfone e a tela abrem em sequência (`bridge.rs:2256-2269`), então o
  disparo pelo app depende de corrida de cliques; o esgotamento não depende de nada.
- **Correção:** uma promessa por pessoa e por sentido (como o `peer.routing`), para chamadas
  simultâneas esperarem a mesma criação; no máximo um transporte por sentido; depois de cada
  `await`, se o transporte fechou ou a pessoa saiu, fechar o producer/consumer novo e responder 404.
- **Dono:** Nimbus.

### P1-4 · Cada `resumeConsumer` de vídeo custa dois quadros-chave

- **Onde:** `sfu/src/Http/Controller/ConsumerController.ts:116-122` — o `consumer.resume()` já
  pede quadro-chave ao producer, e o `requestKeyFrame()` logo depois cai no `keyFrameRequestDelay`
  de 1 s (`ProducerController.ts:13`) e vira um segundo PLI agendado.
- **Cenário:** todo "Assistir", toda volta da janela fora da vista (2 s minimizada já pausa) e todo
  `rewatch` mandam dois PLIs a quem transmite. O `KeyframeGate` dele (`sharing.rs:204-233`)
  espera o espaço e dobra o espaço para 4 s quando os pedidos não param — e quem entra depois
  espera até 4 s pela primeira imagem. Cada quadro-chave é o quadro mais caro, para todos.
- **Prova:** `node sfu/audit/double-keyframe.mjs`
  ```
  primeiro espectador: PLIs (ms depois do resume) [-1,999]; primeiro pacote em 16 ms
  FAIL  primeiro espectador: um resume pede UM quadro-chave  — 2 PLIs
  FAIL  segundo espectador, 6 s depois: um resume pede UM quadro-chave  — 2 PLIs
  ```
- **Correção:** tirar o `requestKeyFrame()` explícito do `resume`.
- **Dono:** Nimbus.

### P1-5 · Banir, expulsar e mutar não chegam ao SFU quando o `/presence` falha ou a pessoa está na carência

- **Onde:** `web/app/Models/Server.php:554-570` (`voiceChannelOf` acha a voz pela presença
  fresca); o SFU tira da presença quem está na carência (`sfu/src/Services/RoomRegistry.ts`).
- **Cenário:** `/presence` lento (2 s) ou com erro: a API responde 2xx e nenhum `kick`/`mute` sai;
  o banido continua assistindo e transmitindo. Banido com o socket recém-caído: não está na
  presença, nenhum `kick` sai, e o RTP dele segue até a carência acabar (indefinidamente para um
  cliente modificado que retome com o token de antes do banimento — a retomada não dispara
  `joined`).
- **Prova:** `bash docs/auditoria/run-web.sh` — `REPRO P1` e `REPRO P1b`.
- **Correção:** no banimento e na expulsão, mandar `kick` para todo canal de voz do servidor, sem
  depender da presença (o `kickUser` é idempotente e já tira quem está na carência); separar
  "presença indisponível" de "sala vazia".
- **Dono:** Nimbus.

### P1-6 · Canal com limite: quem caiu perde o lugar, e quem foi movido para dentro tranca os de dentro

- **Onde:** `web/app/Models/Channel.php:366-368`; a presença não lista quem está na carência.
- **Cenário:** limite 2, Alice cai, Carol entra no lugar, a retomada da Alice leva "O canal está
  cheio." (`REPRO L1`). Limite 1 com alguém movido para dentro: quem já estava não reconecta
  (`REPRO L2`).
- **Prova:** `bash docs/auditoria/run-web.sh` — `REPRO L1`, `REPRO L2`.
- **Correção:** a mesma isenção de "já sentado" do P0-2, ou a presença listar quem está na carência
  com `reconnecting`.
- **Dono:** Nimbus.

### P1-7 · A câmera não volta depois de uma queda e, no macOS, fica ligada sem subir e não religa — provado por leitura

- **Onde:** `native/shared/core/src/room.rs:1220-1237` (`resend`: para a câmera do Linux e solta a
  do macOS sem receita para voltar); `native/apps/macos/Sources/Unkvoid/AppModel+Room.swift:158`
  (só redesenha o `room.mine`) e `native/apps/macos/Sources/Unkvoid/Platform/Camera.swift:54-60`.
- **Cenário:** o `resend` roda quando o servidor fica 5 s calado com pacote saindo
  (`room.rs:276-282`) e na entrada nova depois da carência. Tela e microfone voltam; a câmera não.
  No macOS o Swift não para a captura (a luz verde fica acesa) e, ao religar, o `Camera.start`
  tenta pôr uma segunda entrada na sessão que ainda roda: "Não deu para ligar a câmera" até sair
  da sala.
- **Prova:** leitura de caminho determinístico (o próprio comentário de `room.rs:1223-1224`
  admite); sem câmera neste contêiner para rodar.
- **Correção:** guardar a receita da câmera como a da tela (`shared`) e reabri-la no `resend`; no
  Swift, parar a captura quando `room.mine.camera` vira `false`.
- **Dono:** Stratus (núcleo) e Mirror (Swift).

### P1-8 · macOS: a fila de som de cada pessoa não tem teto, e o atraso só cresce — provado por leitura

- **Onde:** `native/apps/macos/Sources/Unkvoid/Platform/Sound.swift:55`.
- **Cenário:** cada bloco é agendado sem limite nem descarte; o microfone mutado manda silêncio
  (`sharing.rs:1313-1316`), então a fila nunca esvazia. Cada tranco de rede vira atraso
  permanente, e a deriva entre os relógios das duas placas soma minuto a minuto. O vídeo aparece na
  hora: a boca descola da voz com o tempo. O Windows e o Linux já têm folga de 40 ms e teto de
  200 ms (`native/apps/windows/src/sound.rs:31-41`).
- **Prova:** leitura.
- **Correção:** a mesma regra do `sound.rs`: folga inicial e teto, descartando o mais velho aos
  poucos.
- **Dono:** Mirror.

### P1-9 · macOS: entrar numa sala por código estando numa voz deixa a voz viva no Swift — provado por leitura

- **Onde:** `native/apps/macos/Sources/Unkvoid/AppModel.swift:495-531` (`enterRoom` não chama
  `leaveVoice`/`closeRoom`), enquanto o núcleo sai da voz ao entrar (`native/shared/core/src/ffi.rs:95`).
- **Cenário:** na voz → início → uma das "Últimas salas". O `voiceChannel` continua, o chat da voz
  continua inscrito, o microfone continua capturando (bolinha laranja) para uma sala sem producer
  de microfone, e os tocadores e as imagens da voz de antes ficam.
- **Prova:** leitura.
- **Correção:** `enterRoom` passa por `leaveVoice()` antes de pedir a sala.
- **Dono:** Mirror.

### P1-10 · macOS: quadro recusado pelo decodificador espera o quadro-chave periódico — provado por leitura

- **Onde:** `native/apps/macos/Sources/Unkvoid/Platform/VideoSurface.swift:40-47` limpa a camada e
  espera um quadro-chave, mas nenhuma ação do `unkvoid_app` expõe `Room::request_keyframe`
  (`room.rs`).
- **Cenário:** imagem parada até 4 s quando quem transmite é Windows (GOP de 4 s) — o Windows e o
  Linux pedem na hora (`apps/windows/src/watching.rs:332-334`).
- **Prova:** leitura.
- **Correção:** uma ação `requestKeyframe {producerId}` no `ffi.rs`, chamada pelo `VideoSurface`
  quando a camada falha.
- **Dono:** Stratus (ação) e Mirror (chamada).

## P2 — risco

| # | Onde | O quê | Prova | Dono |
|---|---|---|---|---|
| P2-1 | `native/shared/media/src/plain.rs:187`, `:499-507` | Tela e câmera dividem um histórico de reenvio indexado só pelo número de sequência; quando as numerações se cruzam, o NACK da tela reenvia o pacote da câmera e o buraco vira pedido de quadro-chave | `cargo test -p media --test audit a_nack_for_the_screen -- --ignored` → `o NACK pediu o pacote 973 da TELA e voltou o da câmera (SSRC 0x20000002)` | Stratus |
| P2-2 | `plain.rs:495`, `sharing.rs:916-931`, `:1386-1393` | O pedido de quadro-chave é um `bool` do remetente inteiro, sem SSRC: com tela e câmera no ar, quem lê primeiro leva. O Linux passou a atender PLI (#43): a câmera perde o pedido para a tela e espera o GOP de 1 s | `… a_keyframe_request_reaches -- --ignored` → `o PLI da câmera foi entregue a quem lia pela tela (tela: keyframe: true, câmera: keyframe: false)` | Stratus |
| P2-3 | `native/shared/media/src/recovery.rs:24-28` | Numa ida e volta curta (< 80 ms) os três NACKs do mesmo pacote saem de 40 em 40 ms, e o mediasoup só reenvia um por 100 ms (sem RR do receptor ele não sabe a ida e volta): perdido o único reenvio, o buraco é largado aos 250 ms e a imagem para ~0,7 s. Hoje, com o servidor a ~140 ms, não morde | `node sfu/audit/nack-repeat.mjs` → `FAIL 3 NACKs … a cada 40 ms trazem 3 reenvios — 1 reenvio(s)`; a 120 ms, 3. No SFU real com 3% de perda: `received 3960, recovered 99, lost 4`, 192 de 360 quadros inteiros (`lost_packets_come_back_over_the_real_sfu_rtx`) | Stratus |
| P2-4 | `native/shared/core/src/room.rs:1011-1023`, `session.rs:431-436` | Sair da sala manda `leave` e ninguém fecha o socket (o SFU também não): a sessão segue pingando, o `run` segura o `Room` para sempre e `room.ping` da sala velha chega à interface junto com o da nova. Uma sala zumbi por troca de canal | `… leaving_the_room -- --ignored` → `a sala que saiu continuou pingando o SFU (2 pings em 11 s)`; no macOS, 12 entradas e saídas deixaram 13 sockets abertos e 24 `room.ping` em 12 s | Stratus |
| P2-5 | `plain.rs:566` | `stream.bytes` é `u32` e soma o payload: no build de depuração (`cargo run`) estoura e derruba a thread da captura depois de 4 GiB (~57 min a 10 Mb/s, ~14 min em 4K). O de release só dá a volta | semântica do Rust (`overflow-checks` liga no perfil `dev`) | Stratus |
| P2-6 | `media/src/playout.rs:84-90`, `apps/windows/src/sound.rs:33-37` | A imagem espera até 0,5 s no `Playout` e o som só a folga de 40–200 ms: com perda, a boca descola da voz até 0,5 s por ~10 s | leitura | Stratus |
| P2-7 | `media/src/linux_decoder.rs:74-118` | O decodificador do Linux devolve a imagem do quadro anterior: numa tela parada do Windows (um quadro por segundo) a imagem anda um segundo atrás, e a primeira só aparece no segundo quadro | leitura | Stratus |
| P2-8 | `media/src/receiver.rs:104,267`, `apps/windows/src/sound.rs:99-103` | `retired` e as faixas de som por producer nunca encolhem | leitura | Stratus |
| P2-9 | SFU, `transport.produce`/`consume` | Producer ou consumer criado num transporte que fechou durante o `await` é guardado e anunciado assim mesmo | leitura | Nimbus |
| P2-10 | `sfu/src/Services/Peer.ts:124-130` | `assertCanProduce` roda antes dos `await` do `storePlain`: um `/mute` no meio acha zero microfones e o microfone sobe aberto | leitura | Nimbus |
| P2-11 | `sfu/src/Services/Signature.ts:17` | O token vale 60 s + 30 s de folga, sem uso único: expulso, banido ou movido entra de novo com o token que já tinha (cliente modificado) | leitura | Nimbus |
| P2-12 | `sfu/src/Http/Controller/PeerController.ts:14-26` | Qualquer pessoa da sala remove quem está na carência, e a retomada dela vira entrada nova | leitura | Nimbus |
| P2-13 | `sfu/src/Services/Room.ts:204-208` | Quem transmite e retoma não recebe a lista de quem assiste | leitura | Nimbus |
| P2-14 | `sfu/src/Services/Room.ts:281-301` | O desmutar do servidor retoma também o microfone que a própria pessoa tinha pausado | leitura | Nimbus |
| P2-15 | `sfu/src/Services/Room.ts` (`pickRouter`) | Router novo criado no instante em que o último sai fica sem fechar | leitura | Nimbus |
| P2-16 | `ConsumerController.ts` (`consumePlain`) | O consumer copia as extensões e o enchimento do router (transport-cc): pacote que o app não usa | leitura + 16 pacotes de enchimento no roteiro de base do agente do SFU | Nimbus |
| P2-17 | `web/app/Models/Channel.php:413-420` | Mover cujo `kick` não acha ninguém responde 204, deixa o passe sem `CONNECT` vivo e tranca a origem por 60 s | `REPRO M3` | Nimbus |
| P2-18 | `Channel.php` (`move`) | Dois moderadores movendo a mesma pessoa: ela cai em B e o passe de C continua valendo | `REPRO M4` | Nimbus |
| P2-19 | `web/app/Models/Server.php:492` | `server_mute: 1` grava o mudo e morre em `TypeError` antes de avisar o SFU | `REPRO S1` | Nimbus |
| P2-20 | `Channel.php:368` | `/presence` fora do ar deixa o limite passar | `REPRO P2` | Nimbus |
| P2-21 | `web/app/Http/Middleware/VerifySfuSignature.php:44-46` | O webhook igual no mesmo segundo é tomado por repetição: `joined/left/joined` deixa a pessoa fora da lista (`REPRO W2`); o banido que volta no mesmo segundo não é expulso (`REPRO W1`); a marca dura 300 s e o horário aceita +300 s (`REPRO R1`) | Pest | Nimbus |
| P2-22 | `Channel.php:150-166` | Apagar um canal de voz não expulsa quem está nele, e o `left` deles dá 404 | `REPRO D1` | Nimbus |
| P2-23 | `web/app/Services/Sfu/SfuClient.php:197` | A assinatura usa `PHP_EOL`, que é `\r\n` no PHP do Windows | leitura | Nimbus |
| P2-24 | macOS | Tocador de som por producer nunca é desfeito (`MediaRouter.swift:114` só esquece tudo); quadros de um producer fechado ainda saem da fila; o toque de saída é cortado; "saída padrão do sistema" não faz nada (`Sound.swift:266`); `Camera.stop()` e `sink(for:)` seguram a thread principal; corrida no `converter` (`Sound.swift:175,183`); a tela "1080p" sai em pontos, não em pixels, num Retina (`capture/src/macos.rs:336`) | leitura | Mirror |

## Descartados

| # | Suspeita | Por que não é defeito | Prova |
|---|---|---|---|
| D1 | A volta do número de sequência de 16 bits (e o SRTP de lá e de cá) derruba a tela depois de ~65 mil pacotes; SSRC ou tipo de payload errados no RTP puro | Atravessa o mediasoup de verdade nos dois sentidos, com o SSRC e o tipo que o `consumePlain` devolve | `the_sequence_wrap_crosses_the_real_sfu_both_ways`: `6998 quadros, 140019 pacotes …; 6998 quadros inteiros chegaram`, `lost: 0`; `srtp_survives_the_sequence_wrap_with_reordering` (passa) |
| D2 | NACK e RTX não funcionam entre o app e o SFU | Funcionam: o pacote perdido volta pelo RTX e o `unwrap_rtx` o devolve | `lost_packets_come_back_over_the_real_sfu_rtx`: `recovered: 99` |
| D3 | O vigia "servidor calado há 5 s" dispara à toa em quem só tem microfone | O mediasoup 3.26 manda relatório de áudio a cada ~1 s (`MaxAudioIntervalMs = 1000`) | `worker/include/RTC/RTCP/Packet.hpp:23` |
| D4 | O `moved` faz o app voltar para a origem | Era verdade em `5a33c18`; os merges #43/#45/#46 tratam o `moved` no núcleo, no Slint e no Swift | `a_moved_room_tells_the_destination_and_does_not_rejoin_the_origin` (passa) |
| D5 | Carga: 10 pessoas, 2 telas, a sala dividida entre routers | Todo espectador recebe as duas telas, começando por quadro-chave, e o microfone de todos, inclusive por `pipeToRouter` | `node sfu/audit/load-ten.mjs`: `22/22` telas, `132/132` microfones, 12 pessoas em 2 workers |
| D6 | Mutar o microfone (producer pausado) abre buraco na numeração de quem ouve | O mediasoup ressincroniza a numeração ao retomar | `worker/src/RTC/SimpleProducerStreamManager.cpp:230-256` |
| D7 | Quadro-chave que nunca chega para quem começa a assistir | Primeiro pacote em ~16 ms depois do `resumeConsumer` | `node sfu/audit/double-keyframe.mjs` |
| D8 | PLI e pausa através do `pipeToRouter` | O mediasoup repassa | leitura de `Router.js:644-648` |
| D9 | O mesmo SSRC de novo no mesmo transporte | O mediasoup limpa o estado do SRTP ao fechar o fluxo | leitura de `PlainTransport.cpp:938` |
| D10 | Retomada correndo com o fim da carência | `findOrCreate` e `addPeer` não deixam relógio entrar no meio | leitura |
| D11 | Pessoa fantasma depois de expulsar, mover ou substituir | `release` e `orphanPeer` conferem a identidade | leitura |
| D12 | Guest numa sala de 26 caracteres; `can` não reaplicado na retomada | Recusado; reaplicado | leitura |
| D13 | Contagem de pessoas por router ao dividir a sala | Certa, e a divisão simultânea divide a mesma promessa | leitura + D5 |
| D14 | Assistir producer de outra sala | Tudo é por sala | leitura |
| D15 | Assinatura do token e do HTTP entre Laravel e SFU, byte a byte | Idênticas | `cross-check` contra o `dist/Services/Signature.js` e `DISCARD C2`, `C3` |
| D16 | `room` em maiúsculas ou `exp` diferente de 60 s | `room` é ULID minúsculo e `exp` = agora + 60 | `DISCARD C2` |
| D17 | `can` errado com cargos e sobrescritas | Segue `@everyone`, cargos somados, sobrescrita de membro, administrador e dono | `DISCARD C1` |
| D18 | `user_limit` contando acessos que nunca fecharam | Conta a lista viva do SFU | `DISCARD C5` |
| D19 | Mover sem `MOVE_MEMBERS` nos dois canais ou sem `CONNECT` no destino | Recusado, com hierarquia | `DISCARD C6` |
| D20 | Token da sala por código aceitando ULID de canal | Recusa 26 caracteres e maiúsculas | `DISCARD C4` |
| D21 | Cabeçalho do bloco de mídia na ABI (`unkvoid_next_media`) | Little-endian e limites certos, cópia antes de liberar | leitura de `Core.swift:132-149` |
| D22 | `unkvoid_bytes_free`/`unkvoid_string_free` com tamanho errado ou duas vezes | `into_boxed_slice` iguala capacidade e tamanho; uma vez só | leitura + valgrind no caminho do toque |
| D23 | `IOSurface` da câmera vazando ou liberado duas vezes | `passRetained` +1, `from_raw` sem reter, `Drop` libera | leitura |
| D24 | `unkvoid_call`/`unkvoid_app` na thread que desenha | Tudo passa por `offMain` | leitura |
| D25 | Volta do relógio RTP de 32 bits | O `Playout` acompanha (`the_rtp_clock_turning_over_is_not_a_jump`); o remetente soma com `u128`; o Swift ignora o carimbo | testes existentes + leitura |
| D26 | Troca de resolução no meio da transmissão | Os três remetentes repetem SPS/PPS a cada IDR; o decodificador do Windows trata `MF_E_TRANSFORM_STREAM_CHANGE`; o `VideoSink` refaz a descrição a cada quadro-chave | leitura de `windows_decoder.rs:498-550` |
| D27 | Largura ou altura ímpar | `Quality::fit` arredonda para par | `capture/src/lib.rs:176-182` |
| D28 | Tela parada sem pacote (o SFU derrubaria em 30 s) | O Windows repete a imagem a cada 1 s; o macOS repete a última superfície | `sharing.rs` (`StillFrames`), `capture/src/macos.rs:49-58` |
| D29 | Encoder do macOS com B-frames ou sem SPS/PPS | Tempo real, sem reordenar, quadro-chave a cada `frame_rate` quadros, SPS/PPS na frente de cada IDR | leitura de `media/src/macos.rs:150-235` |
