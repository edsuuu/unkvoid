# O app nativo

Decisão do dono: a interface deixa de ser uma webview e passa a ser escrita na linguagem de
cada sistema. Uma pasta por sistema, para a manutenção de cada um ser independente.

Este arquivo é o desenho. O que já está pronto, o que falta, e por onde começar.

## Por que isto é menor do que parece

A parte cara de um app de voz e tela não é a tela: é a mídia. E **ela já é nativa**.

| Peça | Onde mora | Estado |
|---|---|---|
| Captura de tela (GPU) | `shared/capture` | pronta, nos três sistemas |
| Encoder de hardware | `shared/media` | pronto |
| Envio RTP + SRTP | `shared/media/plain.rs` (`PlainSender`) | pronto |
| Recepção RTP + SRTP | `shared/media/receiver.rs` | pronto — já é o caminho do Linux |
| Áudio Opus | `shared/media/audio.rs` | pronto |

O `receiver.rs` existe justamente para "o app sem WebRTC na janela". Ou seja: o caminho que o
app nativo precisa já está escrito, testado e rodando — falta o resto do app usá-lo.

Do lado do SFU, o protocolo para isso também já existe: `producePlain` e `consumePlain`.

## O que ainda depende da janela web

| O quê | Onde | Saída nativa |
|---|---|---|
| Microfone e câmera | `ui/core/Voice.ts` (`getUserMedia`) | `cpal` (áudio) e `nokhwa` (câmera) |
| Transportes WebRTC | `ui/core/SfuClient.ts` (`mediasoup-client`) | não precisa: usar `producePlain`/`consumePlain` |
| Toda a lógica | `ui/core` (7.004 linhas de TypeScript) | virar um crate Rust, compartilhado |
| As telas | `ui/components` (4.172 linhas de TSX) | reescrever por sistema |

## A estrutura

```
native/
  shared/
    capture/          captura de tela          (existe)
    media/            encoder, RTP, SRTP, Opus (existe)
    core/             NOVO — a lógica do app, compartilhada pelos três
    storage/          NOVO — o estado em disco, na pasta do sistema
  apps/
    macos/            NOVO — Swift + SwiftUI
    windows/          NOVO — Rust + Slint
    linux/            NOVO — Rust + GTK4 (gtk-rs)
    desktop/          o app Tauri de hoje, até a paridade
```

**A regra que faz isto valer a pena:** nada de regra de negócio nas pastas de sistema. Elas
desenham e recebem eventos. Quem sabe o que é uma sala, quem pode falar, quando reconectar e o
que mandar ao SFU é o `core`. Se uma decisão aparecer em `apps/macos/`, ela vai ter de ser
escrita de novo em `apps/windows/` e em `apps/linux/` — e é assim que um app vira três apps
diferentes com os mesmos bugs em lugares distintos.

O Linux sai de graça nesse desenho: GTK em Rust fala com o `core` sem ponte nenhuma.

## A ponte para Swift e C#

O `core` expõe uma superfície pequena e estável: comandos entram, eventos saem.

```
    Swift (macOS)  ─┐
    C# (Windows)   ─┼──► core (Rust) ──► capture · media · SFU
    Rust (Linux)   ─┘
```

A ponte é uma ABI C escrita à mão, e **só o macOS precisa dela**: Linux e Windows são Rust e
usam o `core` como crate. São seis funções que mudam devagar, e um header que se lê de cima a
baixo custa menos que mais um gerador no caminho do build.

## O armazenamento em pasta

Sai o `localStorage` (30 usos hoje), entra arquivo na pasta que cada sistema reserva para o
app:

| Sistema | Onde |
|---|---|
| macOS | `~/Library/Application Support/com.unkvoid.desktop/` |
| Windows | `%APPDATA%\com.unkvoid.desktop\` |
| Linux | `~/.config/com.unkvoid.desktop/` (XDG) |

O que é guardado hoje, e continua sendo: o token do Sanctum, o nome, a sala recente, as salas
anteriores, as preferências de voz, o id da instalação e o estado dos painéis.

O token merece tratamento à parte: em arquivo ele fica legível para qualquer processo do
usuário. O chaveiro do sistema (Keychain, Credential Manager, Secret Service) é o lugar dele.

## A ordem

1. **`shared/storage`** — o estado em disco, com migração do que já existe no `localStorage`
   para ninguém ser deslogado. É a peça que o app de hoje já pode usar.
2. **`shared/core`** — a lógica sai do TypeScript: cliente do SFU, sala, voz, chat, estado.
   Aqui mora o grosso do trabalho, e é o que evita escrever tudo três vezes.
3. **Mic e câmera nativos** — `cpal` e `nokhwa`, o último pedaço que ainda depende da janela.
4. **`apps/linux`** — primeiro, porque GTK em Rust não precisa de ponte: valida o `core` com o
   menor caminho.
5. **`apps/macos`** e **`apps/windows`** — com a ponte, já sabendo que o `core` funciona.
6. **Aposentar o Tauri**, quando os três tiverem paridade.

O app Tauri continua de pé o tempo todo. Nenhum passo acima quebra o que existe hoje.
