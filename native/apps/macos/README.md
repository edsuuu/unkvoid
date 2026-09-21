# Unkvoid no macOS

Swift + SwiftUI. Fala com o núcleo em Rust pela ABI C.

## Rodar

```bash
cargo build -p core-app            # da raiz de native/: gera libcore_app.a
cd apps/macos && swift run Unkvoid ws://127.0.0.1:3000/sfu
cd apps/macos && swift test        # a fachada do núcleo, sem precisar de SFU no ar
```

O endereço do SFU entra por argumento porque o núcleo ainda não lê o `GET /api/config` do
Laravel. Sem argumento, `ws://127.0.0.1:3000/sfu`.

## Onde vai cada coisa

| Pasta | O quê |
|---|---|
| `Sources/UnkvoidCore/` | o header C e o `module.modulemap`. Só muda quando a ABI do `core` muda |
| `Sources/Unkvoid/Core.swift` | a fachada do núcleo: traduz tipos, cuida da memória. **Sem regra de negócio** |
| `Sources/Unkvoid/App.swift` | o `@main`: a janela e o roteador das cinco telas |
| `Sources/Unkvoid/AppModel.swift` | o que a janela observa: pega clique, chama o núcleo, publica o que voltou |
| `Sources/Unkvoid/Models.swift` | o que a ABI devolve, com tipo: servidor, canal, membro, mensagem |
| `Sources/Unkvoid/Screens/` | uma tela por arquivo: `Entry`, `Hub`, `Room`, `Offline`, `Updating`, e `Hub/` com as colunas e os modais |
| `Sources/Unkvoid/Components/` | o que se repete entre telas: `Theme`, `Icon` (os SVGs do `Icon.tsx` e o leitor deles), `Chrome` |
| `Sources/Unkvoid/Platform/` | o que só o macOS tem: menu, bandeja, notificação, permissão de captura |
| `Tests/UnkvoidTests/` | a verificação que quebra se a ponte para o Rust quebrar |

## O que o núcleo ainda não entrega

A `Entry`, o `Hub` e a `Room` estão de pé com o que a ABI responde hoje: `state`,
`createRoom`, `joinRoom`, `leaveRoom`, `recentRooms`, `useServer`, `login`, `register`,
`signOut`, `servers`, `server`, `messages` e `sendMessage`. O que falta abaixo não foi
escrito em Swift de propósito — seria escrever de novo em C# e em Rust depois.

| Falta no `core` | Sem isso, a tela |
|---|---|
| `me` (quem está logado, com o token guardado) | ao reabrir com sessão salva, a barra de baixo diz "Conta conectada" em vez do nome, e a Home não sabe quem é dono do servidor |
| voz: entrar no canal, mutar, ensurdecer, câmera | os botões da barra de baixo nascem apagados, e entrar num canal de voz só avisa que a voz não está ligada |
| quem está na sala (o mapa de peers do `SfuClient.ts`) | o palco da `Room` fica no estado "ninguém está compartilhando" |
| captura e compartilhamento de tela | a `Room` não tem botão de transmitir |
| escrever: criar servidor e canal, cargo, convite, expulsar, banir, auditoria | o `ServerSettings` só lê, e a Home não cria servidor |
| preferência guardada (microfone, saída, teclas) | o aparelho escolhido na barra de baixo vale só enquanto a janela estiver aberta |
| tempo real (Reverb) | mensagem nova só aparece ao reabrir o canal |
| `signOut` e `login` não mexem na tela (`app.show`) | quem move a tela nas duas é o `AppModel`, e não o núcleo — há um `ponytail:` lá |

## O que é do macOS, e não do núcleo

- Permissão de gravação de tela (`CGRequestScreenCaptureAccess`) e de microfone
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

**Um handle, um thread por vez.** `unkvoid_call` pega o `Handle` por `&mut` e
`unkvoid_next_event` consome um `Receiver`: o laço de eventos e uma ação em voo se
encontram de verdade. O `NSLock` dentro de `Core` é o que separa os dois — não é zelo, é o
que impede corrida de dados no Rust.
