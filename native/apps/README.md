# A interface

O app único está provisoriamente em `windows/` e será renomeado para `unkvoid/`. Ele desenha e
traduz ações; não decide regra de negócio.

| Pasta | Linguagem | Como fala com o núcleo |
|---|---|---|
| `windows/` | Rust, em transição para egui | direto, como crate |

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

## Por onde começar uma tela nova

1. Veja se a intenção já existe no `shared/core`.
2. Desenhe a partir do estado que o núcleo entrega.
3. Se precisar de uma decisão que não é visual, pare: ela é do `core`.
