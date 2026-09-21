# Unkvoid no macOS

Swift + SwiftUI. Fala com o núcleo em Rust pela ABI C.

## Rodar

```bash
cd native/apps/macos
./run.sh            # compila o núcleo (Rust) e abre o app (Swift)
./run.sh test       # os testes do núcleo e do app, em série, contra a pilha local
./run.sh app        # monta o build/Unkvoid.app e abre — as permissões ficam no nome do Unkvoid
```

Por baixo é `cargo build -p core-app` (gera a `libcore_app.a` que o Swift liga) e
`swift run Unkvoid`. O `bundle.sh` faz o mesmo em release e assina (ad-hoc, ou
`--sign "Developer ID …"`). O script avisa se o Laravel ou o SFU não estiverem no ar; como
subir os dois está no `CLAUDE.md` da raiz.

Para os testes que entram numa conta, `UNKVOID_TEST_PASSWORD=<senha das contas de teste>`;
para o de compartilhar a tela, `UNKVOID_TEST_SHARE=1` (precisa da permissão de gravação).

Os testes rodam **em série** porque dividem uma pasta de estado e as contas de teste: em
paralelo um grava por cima do outro. Rodar muitas vezes seguidas esbarra em dois limites que
não são defeito — o login do Laravel (`throttle:login`) e as 20 conexões por minuto por IP
do SFU (`SFU_CONNECTIONS_PER_MINUTE`, que se sobe num SFU de teste).

O endereço do SFU vem do `GET /api/config` do Laravel (`UNKVOID_SERVER`, por padrão
`http://127.0.0.1:8000`). Sem Laravel, vale `ws://127.0.0.1:3000/sfu` — a sala por código
não pode depender de conta.

Para desenvolver:

| Variável | Para quê |
|---|---|
| `UNKVOID_SFU=ws://…` (ou o endereço como argumento) | outro SFU, no lugar do que o Laravel anuncia |
| `UNKVOID_STATE_DIR=/pasta` | estado e token numa pasta separada: duas janelas, duas contas |
| `UNKVOID_JOIN=codigo:nome` | abre já dentro de uma sala por código |
| `UNKVOID_OPEN=servidor` ou `servidor:canal` | abre o servidor (e entra na voz do canal), com a conta já guardada |
| `UNKVOID_TEST_PASSWORD=…` | liga os testes de chat e voz, que entram como `ada@teste.local` e `grace@teste.local` |

`cargo run -p core-app --example login -- <servidor> <e-mail> <senha>` guarda um token na
pasta de estado; `--example room -- <sfu> <codigo> share|watch` compartilha ou conta o que
chega, sem interface.

## Onde vai cada coisa

| Pasta | O quê |
|---|---|
| `Sources/UnkvoidCore/` | o header C e o `module.modulemap`. Só muda quando a ABI do `core` muda |
| `Sources/Unkvoid/Core.swift` | a fachada do núcleo: traduz tipos, cuida da memória. **Sem regra de negócio** |
| `Sources/Unkvoid/App.swift` | o `@main`: a janela e o roteador das cinco telas |
| `Sources/Unkvoid/AppModel.swift` | o que a janela observa: pega clique, chama o núcleo, publica o que voltou |
| `AppModel+Room.swift` | a sala aberta: avisos do núcleo, compartilhar, assistir, voz, microfone, câmera |
| `AppModel+Chat.swift` | o tempo real: inscrição nos canais, presença, servidor que mudou |
| `AppModel+Social.swift` | amigos, mensagens diretas, criar servidor, convite |
| `AppModel+Server.swift` | escrever no servidor: nome, ícone, convite, canais, cargos, membros, banidos, auditoria |
| `AppModel+Settings.swift` | preferências guardadas, aparelhos, como o microfone abre, teclas, foto, apelido |
| `ChatRoom.swift` | o chat de um canal: mensagens, resposta, imagens, paginação, não lidas. Há dois — o do canal e o da voz |
| `Preferences.swift`, `Models.swift` | o que se guarda e o que a ABI devolve, com tipo |
| `Screens/` | uma tela por arquivo: `Entry`, `Hub`, `Room`, `Offline`, `Updating` |
| `Screens/Hub/` | as colunas do servidor; `Home/` com salas, amigos e conversas; `Modals/` com configurações do servidor e da conta, canal, cargo, membro, confirmar, apelido e logs |
| `Screens/Room/` | o palco, o seletor de compartilhar e a lista de pessoas |
| `Components/` | o que se repete entre telas: `Theme`, `Icon` (os SVGs do `Icon.tsx` e o leitor deles), `Chrome` |
| `Platform/` | o que só o macOS tem — ver abaixo |
| `Info.plist` | embutido no executável pelo linker: sem o texto de uso, o sistema mata o processo ao pedir microfone ou câmera |
| `bundle.sh` | monta o `Unkvoid.app`. Num `.app` a permissão de tela, microfone e câmera é do Unkvoid, e não do terminal |
| `Tests/UnkvoidTests/` | a verificação que quebra se a ponte para o Rust quebrar |

## A mídia, e quem faz o quê

| Caminho | Núcleo (Rust) | macOS (Swift) |
|---|---|---|
| Compartilhar tela | tudo: ScreenCaptureKit → `IOSurface` → VideoToolbox → RTP (`sharing.rs`). O som do sistema vai junto, sem o app de chamada | o seletor (`ShareModal`) |
| Assistir | SRTP → RTP → quadro H.264 inteiro e Opus → PCM (`watching.rs`, `media/unpack.rs`) | `VideoSurface.swift`: Annex-B → AVCC → `AVSampleBufferDisplayLayer`, que decodifica na placa. `Sound.swift` toca o PCM |
| Microfone | Opus → RTP, mudo, nível (`AudioFeed`) | `Sound.swift`: `AVAudioEngine` com o processamento de voz do sistema (cancelamento de eco), sem abaixar o som do jogo; converte para 48 kHz estéreo |
| Câmera | VideoToolbox → RTP (`VideoFeed`) | `Camera.swift`: `AVCaptureSession` em NV12 sobre `IOSurface`, que atravessa a ABI sem cópia |

`MediaRouter.swift` é a thread única que esvazia `unkvoid_next_media`. Nada de mídia passa
pela main: 60 quadros por segundo na thread que desenha a interface a travariam.

## O que já tem, contra o React

Sala por código e voz (entrar, quem está, ping, reconexão e republicar sozinho depois de uma
queda longa); compartilhar tela com seletor e miniaturas, trocar a qualidade no ar, "ver o que
a sala vê"; assistir com foco, tela cheia, pausar, fechar, "Assistir", som e volume por tela,
quem está assistindo; microfone com detecção de voz, apertar para falar e sempre aberto;
câmera; Home com salas, amigos e mensagens diretas; criar servidor e entrar por convite;
canais, cargos, sobrescritas por canal, membros, apelido, expulsar, banir, tirar da voz,
banidos e auditoria; chat com tempo real, responder, editar, apagar, imagens (anexar, colar,
arrastar, ver grande), mensagens antigas ao rolar e não lidas; chat da voz e sala focada;
configurações com barra lateral (conta, voz e vídeo, teclas, notificações) guardadas em disco;
teclas globais que não tiram a tecla do jogo; login com Google; apelido no primeiro acesso;
brilho, contraste, saturação e desfoque por tela; membros online e offline; aviso do sistema
para mensagem direta e pedido de amizade (no `.app`); aviso de versão nova; o relatório de
erro da abertura anterior para o Laravel; logs; o `.app`.

Duas regras que valem para qualquer tela nova:

- **Erro de uma tela não atravessa para outra.** Trocar de tela apaga tudo o que a anterior
  avisava (`AppModel.forgetErrors`), e cada erro tem o lugar dele: o campo, a sala, ou o aviso
  do topo, que é de todas as telas e some com a troca.
- **Quem é tirado da sala não volta sozinho.** `replaced` (a conta entrou por outro lugar) e
  `kicked` encerram a sessão no núcleo; a interface leva a pessoa de volta com o motivo.

## O que ainda falta

| Falta | Por quê |
|---|---|
| instalar a atualização sozinho | o app **avisa** que há versão nova (`/downloads/latest.json`) e abre o instalador; baixar, conferir a assinatura e trocar o `.app` sem a pessoa fazer nada depende de o `release.yml` publicar o app nativo (`darwin-aarch64`), que hoje publica o do Tauri |
| assinatura com Developer ID e notarização | o `bundle.sh` assina ad-hoc; `--sign` resolve a assinatura, e a notarização é um passo do `release.yml` |

## O que é do macOS, e não do núcleo

- Permissão de gravação de tela, de microfone e de câmera (os textos de uso estão no `Info.plist`)
- Decodificar o vídeo (`AVSampleBufferDisplayLayer`), tocar e capturar som (`AVAudioEngine`), capturar a câmera (`AVCaptureSession`)
- Menu da barra, item na bandeja, notificação
- Atalho global (fala-apertando funciona com a janela atrás do jogo)
- Aparência: `NSVisualEffectView`, modo escuro do sistema

## O que nunca vai aqui

Qualquer coisa que o Windows e o Linux também precisariam: o que é uma sala, quem pode
falar, quando reconectar, o que guardar em disco, o formato do que vai ao SFU. Isso é
`shared/core`. Se você escrever e pensar "o Windows vai precisar igual", pare e suba.

## Cuidados

**Memória.** Toda string do `core` volta em `unkvoid_string_free`, uma vez. Use `defer`
logo depois de pegar o ponteiro — ele sobrevive a `throw` no meio do método.

**A tela não espera rede.** `nextEvent()` não bloqueia: consulte num timer ou numa
`AsyncStream`, nunca segurando o desenho.

**SwiftUI não é o dono do estado.** O estado é o que o `core` devolve. A `View` observa e
desenha; ela não guarda regra.

**Sem cadeado deste lado.** Do lado do Rust tudo o que muda vive atrás de cadeado, então
`Core` não tem nenhum: a fila de eventos e a de mídia são lidas enquanto uma ação está em
voo. `unkvoid_next_media` é para **uma** thread só — é a do `MediaRouter`.

**O layer é da main.** O `AVSampleBufferDisplayLayer` nasce e é posicionado na main; quem
recebe quadro de outra thread é o `sampleBufferRenderer` dele. Quadro que chega antes de o
cartão existir fica guardado do último keyframe em diante (`MediaRouter`).

**Não toque no `inputNode` à toa.** Só encostar nele já pede o aparelho ao sistema, e numa
saída de sala isso custava 45 s. `Sound.mute()` só mexe nele se o microfone foi aberto.
