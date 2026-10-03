# As interfaces

Uma pasta por sistema. Cada uma desenha; nenhuma decide.

| Pasta | Linguagem | Como fala com o núcleo |
|---|---|---|
| `windows/` | Rust + Slint | direto, como crate |
| `linux/` | Rust + GTK4 | direto, como crate |
| `macos/` | Swift + SwiftUI | ABI C (`shared/core/src/ffi.rs`) |
| `desktop/` | React + Tauri | o app de antes: referência de comportamento, não é mais publicado |

## A regra

**Regra de negócio não mora aqui.** Quem sabe o que é uma sala, quem pode falar, quando
reconectar, o que mandar ao SFU e o que guardar em disco é o `shared/core`. Se uma linha escrita
aqui vai precisar ser escrita de novo nas outras pastas, ela está no lugar errado.

O que pertence a cada pasta de sistema:

- desenhar a tela a partir do estado que o `core` entrega;
- traduzir clique, tecla e arrasto em chamada ao `core`;
- escrever em português a frase de cada motivo de erro que o `core` devolve (`Failure`,
  `EntryRefusal`, `room.failed`);
- o que só aquele sistema tem: bandeja, notificação, atalho global, permissão de captura, os
  aparelhos de som.

## A ponte do macOS

As funções da ABI C e o que cada uma faz estão em
[docs/ARQUITETURA.md](../../docs/ARQUITETURA.md#a-ponte-para-o-swift). Comandos e avisos falam
JSON. `unkvoid_call` e `unkvoid_app` bloqueiam — nunca na thread que desenha — e toda string
devolvida volta em `unkvoid_string_free`, uma vez só.

## Por onde começar uma tela nova

1. Veja o que o React faz em `desktop/ui/components/` — é a referência de comportamento, não
   de código. As medidas exatas estão em `desktop/ui/style.css`.
2. Se ele chama algo de `desktop/ui/core/`, esse algo vira (ou já é) função do `shared/core`.
3. Desenhe. Se precisar de uma decisão que não é visual, pare: ela é do `core`.
