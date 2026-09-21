# As interfaces

Uma pasta por sistema. Cada uma desenha; nenhuma decide.

| Pasta | Linguagem | Como fala com o núcleo |
|---|---|---|
| `macos/` | Swift + SwiftUI | ABI C (`shared/core/src/ffi.rs`) |
| `windows/` | Rust + Slint | direto, como crate — sem ponte |
| `linux/` | Rust + GTK4 | direto, como crate — sem ponte |
| `desktop/` | React + Tauri | o app de hoje, até haver paridade |

## A regra

**Regra de negócio não mora aqui.** Quem sabe o que é uma sala, quem pode falar, quando
reconectar, o que mandar ao SFU e o que guardar em disco é o `shared/core`.

O teste é simples: se você escrever uma linha em `macos/` que precisará ser escrita de novo
em `windows/` e em `linux/`, ela está no lugar errado. Sobe para o `core`.

O que pertence a cada pasta de sistema:

- desenhar a tela a partir do estado que o `core` entrega
- traduzir clique, tecla e arrasto em chamada ao `core`
- o que só aquele sistema tem: menu nativo, bandeja, notificação, atalho global, permissão
  de captura

## O contrato

O `core` expõe cinco funções, e só elas:

```c
Handle *unkvoid_core_new(void);
bool    unkvoid_connect(Handle *, const char *url);
char   *unkvoid_call(Handle *, const char *action, const char *data_json);
char   *unkvoid_next_event(Handle *);
void    unkvoid_string_free(char *);
void    unkvoid_core_free(Handle *);
```

Comandos entram por `unkvoid_call`, eventos saem por `unkvoid_next_event`. Os dois falam
JSON, porque é o que as três linguagens leem sem esforço e o que já trafega na rede.

**Memória:** toda string devolvida pelo `core` volta em `unkvoid_string_free`, uma vez só.
Em Swift isso fica atrás de um `defer`; em C# atrás de um `try/finally`.

**Eventos não bloqueiam a tela.** `unkvoid_next_event` não espera: devolve o próximo da fila
ou nada. A interface consulta no ritmo do desenho.

## As telas

São as mesmas cinco em qualquer sistema, e o `core` diz qual está valendo:

| Tela | Quando |
|---|---|
| `Entry` | sem conta: nome, criar sala, entrar por código; e o login |
| `Hub` | com conta: servidores, canais, chat, membros, voz |
| `Room` | dentro de uma sala ou canal de voz: quem está, quem transmite |
| `Offline` | o servidor não respondeu |
| `Updating` | baixando versão nova |

O detalhe de cada uma está no `README.md` da pasta do sistema.

## Por onde começar uma tela nova

1. Veja o que a versão React faz em `desktop/ui/components/` — ela é a referência de
   comportamento, não de código.
2. Se ela chama algo de `desktop/ui/core/`, esse algo vira (ou já é) função do `shared/core`.
3. Desenhe. Se precisar de uma decisão que não é visual, pare: ela é do `core`.
