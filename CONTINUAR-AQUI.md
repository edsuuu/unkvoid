# Onde paramos, e o que falta validar

Escrito em 20/09/2026, na branch `app-nativo`. **Nada foi commitado.**

Este arquivo existe para quem pegar o trabalho depois — não repita o que já está de pé, e
não confie no que está marcado como não verificado.

## O que está pronto e **verificado**

| O quê | Prova |
|---|---|
| `shared/core` — protocolo e cliente do SFU, sessão, a sala viva da ABI, assistir sem GStreamer, API do Laravel com o mapa de rotas, permissões, portão do microfone, login com Google, preferências, mapa de teclas, ABI C | 92 + 6 testes, clippy limpo |
| `shared/storage` — estado em disco na pasta do sistema, token cifrado em AES-256-GCM | 14 testes |
| App Windows (Slint) | 4 testes, clippy limpo, **janela aberta e conferida aqui** |
| App Linux (GTK) | 133 testes no contêiner, janela abrindo sob `xvfb`, **mídia ligada** |
| App macOS (SwiftUI) | 21 testes em série; cobre o que o React tem — ver `native/apps/macos/README.md` |
| Ponte Swift → Rust → SFU | `swift run` conecta e recebe resposta, rodado |
| Tempo real do SFU (identify, subscribe, broadcast, presença) | 10 verificações em `sfu/check-realtime.mjs` |
| Laravel publicando pelo SFU, sem Reverb | 123 testes, phpstan max, MySQL |
| App Tauri com o tempo real novo | 151 testes |
| Bind não engole mais a tecla do jogo no macOS | compila e passa nos testes; **falta apertar a tecla com um jogo na frente** |

## O que **não** foi verificado, e por quê

### Windows
Refeito em **Rust + Slint** (decisão do dono, 20/09/2026) — o C# foi apagado. Como o Slint
roda no macOS, **isto foi verificado aqui**: clippy limpo, testes verdes, e a janela abriu com
as cinco telas desenhando. O que só um Windows confirma é a lista de aparelhos pelo WASAPI e o
visual final naquele sistema.

```bash
cargo run -p unkvoid-windows
```

### Linux
**Não compila nesta máquina** — o GTK4 não existe no macOS. Há um `Dockerfile` em
`native/apps/linux/` que compila e roda sob `xvfb`. Para validar:

```bash
cd native && docker build -f apps/linux/Dockerfile -t unkvoid-linux . && docker run --rm unkvoid-linux
```

### macOS
O app nativo cobre o que o React tem (21/09/2026) — a lista inteira e o que falta estão em
`native/apps/macos/README.md`. **Verificado aqui**, com a pilha local no ar:

- a tela de uma pessoa desenhando na janela de outra, decodificada pelo
  `AVSampleBufferDisplayLayer` (218 quadros em 8 s a 30 fps);
- 23 testes do Swift em série (`./run.sh test`), entre eles: a Ada compartilha a tela pela
  mesma ação do botão e a Grace recebe quadros H.264 inteiros, o primeiro um keyframe; duas pessoas na mesma sala se veem; a mensagem da
  Ada chega ao socket da Grace pelo tempo real; a Ada fala na voz e a Grace recebe o som já
  decodificado; um servidor é criado, ganha canal e cargo, muda de nome e é apagado; uma
  mensagem é enviada, respondida, editada e apagada pelo `ChatRoom`; a preferência sobrevive a
  reabrir o app;
- 95 + 8 testes do núcleo (rotas, permissões, portão do microfone, login com Google, teclas,
  relatório de erro, e a sessão substituída que **não** volta sozinha);
- capturas da Home, do servidor e do modal de apelido conferidas a olho;
- o `bundle.sh` gera o `Unkvoid.app` assinado ad-hoc.

**Não verificado**, porque precisa de gente na frente: microfone e câmera de verdade (abrir o
aparelho pede permissão na tela), o som saindo no fone, as teclas globais com um jogo na
frente, o login com Google (precisa do navegador e de uma conta), e cada modal clicado à mão
— o que está por baixo deles tem teste; o desenho de cada um, não.

Um achado de caminho: o SFU cujos workers do mediasoup morrem continua respondendo `ok` no
`/health`, e todo `join` dá 500 (`Channel closed … WORKER_CREATE_ROUTER`). Foi o estado em que
o SFU local estava duas vezes nesta sessão. O `/health` devia conferir os workers.

## O buraco que atrasou tudo, e o que já foi tapado

Os três agentes chegaram, sozinhos, à mesma conclusão: o `shared/core` era só um cano de
rede, e toda tela esbarrava numa decisão que não existia nele.

Já resolvido depois disso:

- `createRoom`, `joinRoom`, `leaveRoom`, `state`, `recentRooms` — pela função
  `unkvoid_app` da ABI, com teste que percorre o caminho inteiro
- código de sala: sortear e validar, com a mesma regra do servidor
- o token do Sanctum, guardado cifrado

Tudo aquilo foi feito: cliente da API, `Roster` da sala, identidade do `join`, reconexão com
`resumeKey`, ping de 5 s, e mic/câmera pelo `shared/capture` (sem `cpal` nem `nokhwa`).

**Ainda falta no `core`**, e cada item trava algo:

| Falta | Trava |
|---|---|
| a captura atravessar a ABI | compartilhar tela no macOS |
| o `Roster` atravessar a ABI | saber quem está na sala, no macOS |
| as preferências de voz (modo de mic, sensibilidade, ruído, teclas) | a tela de configuração nas três |
| o fluxo do Google (`unkvoid://`) | entrar com Google fora do Tauri |

## Duas armadilhas da ABI, já documentadas mas fáceis de esquecer

1. **`unkvoid_call` bloqueia.** Por dentro é `block_on`: chamada da thread que desenha,
   congela a janela. Toda interface tem de chamá-la de fora da thread da interface.
2. **Toda string devolvida volta em `unkvoid_string_free`**, uma vez só — no Swift, atrás de
   um `defer`.

Só o macOS passa por essa ponte. Linux e Windows usam o `core` como crate.

O `Handle` já é seguro para uso concorrente (tudo atrás de `Mutex`, funções por `&`) — isso
foi corrigido depois que dois agentes acharam a corrida de dados, cada um por conta.

## O que o review achou e já está corrigido

| Onde | O quê |
|---|---|
| `shared/core/app.rs` | duas chaves divergiam do que o app de hoje gravou: quem trocasse do Tauri para o nativo viraria outra instalação para o SFU e perderia a última sala |
| `shared/core/api.rs` | o nome do campo se perdia no erro de validação — o e-mail ficava vermelho e a frase aparecia embaixo da senha |
| `shared/core/ffi.rs` | `register` não existia; "Criar conta" dava erro genérico |
| `shared/core/client.rs` | `connect_async` sem prazo: servidor que aceita e não fala pendurava o app |
| `web/User.php` | o e-mail de "novo acesso" saía em **todo** login, do mesmo IP de sempre |
| `apps/linux` | `install_id` e a ordenação de canais duplicavam o núcleo |

## Uma decisão em aberto

As três interfaces traduzem os mesmos motivos nas **mesmas frases em português** —
`unreachable` → "Não deu para falar com o servidor" está escrito três vezes. Foi decisão de
desenho (o núcleo dá o motivo, a interface escreve a frase), e o `match` exaustivo faz motivo
novo virar erro de compilação nas duas UIs em Rust. Mas se o produto for só em português,
isso não se paga: a frase caberia no núcleo. **Decisão do dono**, e vale uma linha aqui seja
qual for.

## O que o dono pediu e ainda não foi feito

- Comparar tela a tela com o React e deixar o design idêntico (tamanho, proporção, input,
  botão), e usar vidro nos botões do macOS
- Olhar a tela de configuração do Discord e replicar: binds, voz e vídeo, notificações, o
  modal com barra lateral, e o popover da setinha ao lado do mic e do áudio
- A tela de configuração no molde do Discord: o **popover da setinha** está feito nas três,
  mas o **modal com barra lateral** só existe para servidor; o de usuário é lista simples, e
  falta o conteúdo (mic, sensibilidade, ruído, teclas) porque as preferências não subiram
  para o núcleo
- **Validar o compartilhamento de tela ponta a ponta** — nenhum app nativo provou isso:
  o macOS não tem captura, e a do Linux nunca rodou contra um SFU de verdade
- Revisão de código e revisão de over-engineering, no fim
- Remover React, TypeScript e JavaScript da interface — **só depois** de as três terem
  paridade, senão o produto fica sem interface nenhuma
