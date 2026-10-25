# Voz

Desenho para revisar antes de implementar. O app atual já abre o microfone, aplica o modo de
entrada e envia Opus. A tela completa de voz, os filtros selecionáveis e os sons ainda não
existem como conjunto. As referências abaixo apontam para o código que existe hoje.

## Caminho do microfone

No Windows, [`Microphone::start`](../native/apps/unkvoid/src/sound.rs#L290) abre uma thread e
entrega cada bloco PCM ao callback. A captura WASAPI copia os quadros em
[`record`](../native/apps/unkvoid/src/sound.rs#L509) e chama `sink` na linha 526. A lista de
aparelhos vem de [`devices.rs`](../native/apps/unkvoid/src/devices.rs#L28); a captura aceita um
aparelho escolhido ou o padrão.

No Linux, [`sound::capture`](../native/apps/unkvoid/src/sound.rs#L727) usa
`PlatformCapturer`. O microfone entra pelo `pulsesrc`, em
[`capture/linux.rs`](../native/shared/capture/src/linux.rs#L656). Quando o plugin está instalado,
o pipeline atual já pode aplicar `webrtcdsp` para redução de ruído e ganho
([linhas 660–672](../native/shared/capture/src/linux.rs#L660)). `devices.rs` escolhe o microfone
trocando a fonte padrão do PulseAudio; o `pacat` é a saída de áudio, não a entrada
([`sound.rs`](../native/apps/unkvoid/src/sound.rs#L610),
[`devices.rs`](../native/apps/unkvoid/src/devices.rs#L51)). Hoje, escolher outro microfone no Linux
troca a fonte padrão do sistema. A opção de aparelho por aplicativo ainda precisa deixar de ter
esse efeito colateral. Ao transferir os filtros para o núcleo, o `webrtcdsp` de entrada também
precisa ser desligado para não processar duas vezes.

O núcleo abre o producer de voz em [`Room::open_microphone`](../native/shared/core/src/room.rs#L921).
O cliente entrega o PCM por `Room::speak`, estéreo `f32` a 48 kHz
([linhas 981–984](../native/shared/core/src/room.rs#L981)). Isso chega a
[`AudioFeed::push`](../native/shared/core/src/sharing.rs#L1326), que mede o nível, aplica mudo e
o gate, e passa os samples ao encoder. [`AudioEncoder::push`](../native/shared/media/src/audio.rs#L59)
acumula os pedaços variáveis e produz quadros Opus de 20 ms
([linhas 6–13](../native/shared/media/src/audio.rs#L6)).

O ponto de inserção é entre a captura e o gate/Opus: PCM de entrada → quadros de processamento
de 10 ms → filtros → gate/mudo → acumulador Opus de 20 ms → SFU. A captura Windows chama o
callback na thread que lê WASAPI; não se deve executar modelos nela. O callback deve copiar para
uma fila limitada e um worker do núcleo processa os quadros, chama `Room::speak` e descarta o
bloco mais antigo se a fila atrasar. Assim a captura não bloqueia e a latência não cresce sem
limite. O medidor de entrada pode continuar lendo o sinal anterior aos filtros, para mostrar o
nível físico e para a detecção de voz não depender do ganho automático.

## Núcleo de processamento

A regra fica em `native/shared/core/src/voice/` (módulo novo), não em
`native/apps/unkvoid/`: formatos, estado de filtro, ordem, configuração, gate e política de
latência pertencem ao core. O app limita-se a selecionar aparelho, entregar PCM, desenhar os
controles e encaminhar eventos de tecla.

Pipeline proposto:

1. **Sonora / WebRTC APM**: AEC3, passa-alta e AGC2. O cancelador de eco recebe também a
   referência do áudio que o app toca; sem essa referência o AEC não tem como reconhecer o eco
   dos alto-falantes. A supressão WebRTC padrão fica como um modo alternativo de redução de ruído,
   não empilhada sobre outro supressor.
2. **Um supressor escolhido**, nunca todos em série: DeepFilterNet3 para a opção de maior
   redução; RNNoise (`nnnoiseless`) para o perfil leve; ou nenhum. O perfil Estúdio desliga o
   processamento não essencial. Isolamento de Voz só entra quando houver um modelo que passe os
   testes manuais. O Krisp do Discord é proprietário e não será copiado nem usado.
3. **Atenuação**: no Discord é o volume dos **outros aplicativos** (o jogo, a música) que baixa
   na porcentagem escolhida enquanto alguém fala — "quando eu falo", "quando outros falam", ou os
   dois. Não mexe no microfone nem na voz de quem está na chamada. É um laço à parte do microfone:
   a detecção de fala (a minha e a dos outros, que o `Watch` já marca) liga e desliga a redução, e
   quem reduz é o sistema — no Windows, o volume de cada sessão de áudio dos outros processos
   (`IAudioSessionManager2`/`ISimpleAudioVolume`), no Linux o volume de cada `sink-input` do
   PulseAudio. Ao sair da voz, ou o app fechar, os volumes voltam ao que eram.
4. **Gate e envio**: modo voz, apertar para falar ou aberto; depois, Opus. O atraso de soltar é um
   temporizador do gate, não um filtro de áudio.

Sonora exige quadros de 10 ms: a 48 kHz são 480 amostras por canal. A medição publicada pelo
[projeto sonora](https://github.com/dignifiedquire/sonora/blob/main/BENCHMARKS.md) é 22,3 µs por
quadro estéreo de 48 kHz para AEC3 + supressão + AGC2 em um Apple M4 Max, aproximadamente 0,22%
de um núcleo naquele equipamento. Não é uma promessa para os computadores do Unkvoid.

DeepFilterNet3 trabalha a 48 kHz, FFT de 960 amostras e avanço de 480 (10 ms); o modelo usa
lookahead, portanto acrescenta latência. O artigo do
[DeepFilterNet3](https://arxiv.org/abs/2305.08227) publica fator de tempo real de 0,19 em CPU de
notebook de uma thread: cerca de 190 ms de CPU por segundo de áudio, ou 19% de um núcleo na
máquina do artigo.

RNNoise recebe mono, 48 kHz, 480 amostras por quadro (10 ms). A saída estéreo deve ser downmixada
e depois recomposta sem deslocar os canais; isso precisa de teste auditivo. O comparativo
publicado para `nnnoiseless` registra 27,6 µs por quadro de 10 ms (cerca de 0,28% de um núcleo
naquela medição) no
[benchmark do port Rust](https://github.com/lovelace-labs/rnnoise-rs#better-than-nnnoiseless).
É uma medição pequena e dependente de CPU; o ganho real precisa ser medido no Windows e no Linux.

`hush-vani` trabalha em mono a 16 kHz, 160 amostras/10 ms, com uma janela de latência de 10 ms;
converter 48 kHz para 16 kHz e voltar também tem custo. A
[documentação da crate](https://docs.rs/hush-vani/latest/hush_vani/) não publica um benchmark de
CPU comparável. Fica fora da primeira seleção até passar em gravações reais e em medição local.

Os números vêm de processadores e métodos diferentes. São referência para priorizar o protótipo,
não se somam nem substituem uma medição do app.

## Tela e preferências

Em **Voz e vídeo → Voz**, a tela terá seleção do microfone e do alto-falante por aparelho,
volume de entrada e saída, e teste de microfone com medidor. Os três perfis de entrada serão
Isolamento de Voz, Estúdio e Personalizado.

Em **Personalizado**: sensibilidade automática ligada/desligada e limiar; supressão de ruído
(WebRTC padrão, RNNoise leve, DeepFilterNet3 ou nenhuma); cancelamento de eco; modo aberto ou
apertar para falar e atraso para soltar; AGC2; detecção de voz avançada; contorno do processamento
do sistema; aviso de aparelho novo e de áudio não detectado; aviso ao mudar de canal; atenuação,
com intensidade e opções para fala local e dos outros; qualidade de serviço.

O núcleo já oferece armazenamento genérico em `App::preference` e `set_preference`
([`app.rs`](../native/shared/core/src/app.rs#L77)). `InputMode` já entende `voice`, `ptt` e
`open`, e cita `unkvoid:voice` como formato
([`sharing.rs`](../native/shared/core/src/sharing.rs#L1263)). O `unkvoid:voice` já gravado nas
máquinas é um **objeto** (`mode`, `sensitivity`, `muteOnJoin`, `echoCancel`, `noiseSuppress`,
`autoGain`, `camera`): os campos novos entram nele, e os de hoje mantêm o nome e o sentido. Um
campo ausente vale o padrão; um valor que esta versão não conhece também.

| Opção | Campo em `unkvoid:voice` | Valor |
|---|---|---|
| Microfone e alto-falante | `microphone`, `speaker` | id estável do sistema; ausente é o padrão |
| Volume de entrada e saída | `microphoneVolume`, `speakerVolume` | 0 a 200, como o Discord |
| Perfil | `profile` | `isolation`, `studio` ou `custom` |
| Modo e sensibilidade (já existem) | `mode`, `sensitivity`, e `autoSensitivity` novo | `open`/`ptt`/`voice`, 0–100, booleano |
| Supressão de ruído | `noise` (o `noiseSuppress` de hoje vira `deep` quando verdadeiro) | `deep`, `light`, `webrtc` ou `off` |
| Eco e ganho (já existem) | `echoCancel`, `autoGain` | booleano |
| Avançado | `advancedVad`, `bypassSystem`, `deviceSuggestion`, `noAudioWarning`, `confirmSwitch`, `qos` | booleano |
| Apertar para falar | `pttBinding`, `pttReleaseMs` | tecla ou botão + modificadores; atraso em ms |
| Atenuação | `ducking` | `{ strength: 0–100, whenISpeak, whenOthersSpeak }` |

As chaves de token continuam fora das preferências comuns: `unkvoid:token` e
`unkvoid:refresh` usam o caminho cifrado em `App`. A preferência de aparelho deve tolerar um id
que desapareceu e voltar ao padrão, sem travar a entrada.

## Apertar para falar

`Room::talk(bool)` já comunica ao gate a descida e a subida da tecla
([`room.rs`](../native/shared/core/src/room.rs#L974)); `InputMode::PushToTalk` só abre enquanto
`talking` está ligado e o gate atual usa uma cauda fixa de 350 ms
([`sharing.rs`](../native/shared/core/src/sharing.rs#L1284)). O atraso configurável da interface
deve substituir essa constante no modo PTT. Na soltura, agendar o fechamento sem dormir na thread
de entrada; se apertar de novo durante a cauda, cancelar o fechamento.

No Windows, `native/shared/clips/src/hotkeys.rs` já tem captura de combinação, modificadores,
callback em thread própria, `set_active` e teste de tecla presa/conflito
([Hotkeys](../native/shared/clips/src/hotkeys.rs#L168)). Reaproveitar o modelo de combinação e o
fluxo de captura, ampliando a entrada para distinguir tecla e botão do mouse. Não registrar PTT
como hotkey global que rouba a tecla do jogo: precisa detectar pressionamento e soltura enquanto
o jogo está em foco.

No Linux, preservar a mesma API e eventos `Pressed`/`Released`, com backend da plataforma
separado. PTT deve funcionar com botão de mouse. Acesso global de teclado varia entre X11,
Wayland e permissões do dispositivo; quando o backend não puder observar a soltura, a interface
deve avisar e oferecer o modo de voz, em vez de deixar o microfone aberto. O core recebe apenas
os eventos e não conhece a API do sistema.

## Sons e eventos

O inventário do Discord tem 22 sons de voz, mais os sons de notificação de mensagem e chamada.
São nomes e eventos, não arquivos para copiar. Cada evento abaixo é a proposta de gatilho para
definir no contrato de eventos do app:

| Som | Evento que deve disparar | Hoje |
|---|---|---|
| 1. Áudio desativado | preferência de saída muda de ativo para desativado | não existe |
| 2. Áudio reativado | preferência de saída muda de desativado para ativo | não existe |
| 3. Microfone silenciado | clique local ou producer `mic` pausado | não existe |
| 4. Microfone dessilenciado | clique local ou producer `mic` retomado | não existe |
| 5. Câmera ligada | producer `camera` criado | não existe |
| 6. Câmera desligada | producer `camera` fechado | não existe |
| 7. Voz desconectada | sessão da sala passa a `gone` ou socket termina sem retomada | não existe |
| 8. Apertar para falar ativado | a tecla do apertar para falar desce (o microfone abre) | não existe |
| 9. Apertar para falar desativado | a tecla sobe e a cauda termina (o microfone fecha) | não existe |
| 10. Usuário entrou | `peerJoined` do roster | existe: `Chime::Joined` |
| 11. Usuário saiu | `peerLeft` do roster | existe: `Chime::Left` |
| 12. Usuário movido | evento `room.session` com estado `moved` | não existe |
| 13. Toque de chamada | evento de chamada recebida passa a tocar / para de tocar | não existe |
| 14. Transmissão iniciada | producer `screen` criado | existe: `Chime::StreamStarted` |
| 15. Transmissão interrompida | producer `screen` fechado | existe: `Chime::StreamStopped` |
| 16. Espectador entrou | pessoa começa a assinatura de uma transmissão | não existe |
| 17. Espectador saiu | pessoa termina a assinatura de uma transmissão | não existe |
| 18. Atividade começou | evento de início de atividade no canal | não existe |
| 19. Atividade terminou | evento de fim de atividade no canal | não existe |
| 20. Usuário entrou na atividade | participante adicionado à atividade | não existe |
| 21. Usuário saiu da atividade | participante removido da atividade | não existe |
| 22. Convite para falar | convite de fala recebido | não existe |

As quatro mudanças de presença/transmissão são sintetizadas no núcleo. A enum
[`Chime`](../native/shared/core/src/chimes.rs#L19) tem ainda `Message`; o realtime já atribui esse
som a mensagens recebidas ([`realtime.rs`](../native/shared/core/src/realtime.rs#L271)). Logo,
existem hoje cinco sons: entrada, saída, transmissão iniciada, transmissão interrompida e
mensagem. O teste de ordem e transições fica em
[`chimes.rs`](../native/shared/core/src/chimes.rs#L168). Não há som de chamada implementado.

Mensagem e chamada também precisam de sons de notificação fora da sala:

| Notificação | Evento que deve disparar | Hoje |
|---|---|---|
| Mensagem | mensagem de canal ou direta recebida enquanto a conversa não está aberta | existe como `Chime::Message` |
| Chamada | chamada recebida ou chamada perdida, conforme a tela | não existe |

Os sons continuam sintetizados no core: senoides,
ataque curto e cauda exponencial, como os cinco de `chimes.rs`. Não usar gravações, melodias ou
amostras do Discord.

## Testes

Sem hardware, cada peça deve ganhar testes determinísticos:

- O adaptador de quadros deve receber blocos de captura de tamanhos diferentes, formar quadros
  completos de 10 ms sem perder nem duplicar amostras, respeitar o limite da fila e recuperar-se
  de uma fila cheia sem aumentar a latência.
- Cada filtro deve receber silêncio, tom, fala sintética e ruído reprodutível; verificar saída
  finita, limites, taxa de amostras, estado entre quadros e diferença esperada entre ligado e
  desligado. AEC exige uma referência de reprodução simulada.
- A configuração deve testar todas as opções, defaults, valor inválido, preferência ausente,
  aparelho que sumiu e migração de versão. A chave antiga `unkvoid:voice` deve continuar sendo
  interpretada.
- Gate e PTT: tecla pressionada abre, soltura respeita atraso, nova pressão cancela a cauda,
  mudo e mute do servidor prevalecem, perda de eventos não deixa o microfone aberto.
- O medidor deve ser estável em silêncio e em tons de nível conhecido. O encoder deve continuar
  emitindo quadros Opus de 20 ms após o worker entregar os quadros processados.
- Sons: tabela de evento→som, prioridade de entrada/saída/transmissão, nenhuma repetição na
  primeira lista e cobertura das cinco chimes atuais. Os novos eventos devem ter testes sem
  tocar dispositivo algum.
- Hotkeys: combinação salva e capturada, tecla e botão do mouse, pressionar/soltar, conflito,
  cancelamento, perda de foco e callback de atraso, tudo com backend falso.

Roteiro manual: gravar a mesma frase e pausas com teclado mecânico, ventilador e TV ao fundo;
repetir cada tomada sem filtro, com WebRTC, DeepFilterNet3 e RNNoise. Fazer a comparação também
com o Discord, no mesmo microfone, distância, ganho e volume de reprodução. Igualar o volume das
gravações antes de comparar. Ouvir inteligibilidade, voz metálica, sílabas cortadas e quanto do
ruído permanece. Medir CPU, latência de ponta a ponta e atraso de soltura no Windows e no Linux;
repetir em CPU modesta. Testar eco com alto-falante, fone e troca de aparelho. Só então decidir
quais perfis saem do experimental.

## Fora desta fase

- Krisp e qualquer som ou modelo proprietário do Discord: não temos licença nem precisamos
  imitar a implementação fechada.
- Isolamento por `hush-vani`: só depois de medir custo, latência, qualidade e distribuição do
  modelo. O formato de 16 kHz também obriga conversão.
- AGC e supressão duplicados: em cada configuração deve existir uma cadeia clara; o filtro de
  ruído do sistema não pode ficar ativo junto com o filtro escolhido pelo core.
- AEC completo sem referência confiável do áudio de saída: o core precisa receber a reprodução
  para cancelar eco. Até isso existir, mostrar o recurso como indisponível, não prometer o
  cancelamento.
- Voz no servidor, gravação/transcrição, áudio espacial, efeitos de voz e alteração do protocolo
  SFU: não são necessários para a tela local de voz.
- Sons de atividade e chamada antes dos eventos correspondentes existirem. O som não deve
  inventar um estado de produto.
- Alterar volume global do sistema como efeito colateral. A escolha do microfone no Linux hoje
  altera a fonte padrão; a tela nova deve tornar essa limitação clara ou esperar suporte a uma
  fonte por aplicativo antes de prometer seleção independente.
