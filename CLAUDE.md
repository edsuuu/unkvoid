# Unkvoid

Compartilhar a tela com quem você mandar o código, **sem perder fps no jogo** — e, para quem
tem conta, servidores com canais de texto e voz, câmera e chat, no molde do Discord.

O objetivo manda em tudo: `captura → textura na GPU → encoder de hardware → 1 quadro → SFU → N
espectadores`. O quadro não desce para a memória do processador antes de ser comprimido, é
comprimido **uma vez** e sobe **uma vez**. Mudança que quebre uma dessas três coisas desfaz o
projeto (no navegador o encoder roda na CPU e a transmissão cai para 1 fps com o jogo aberto).

## As três peças

| Pasta | O quê | Onde roda | Agente |
|---|---|---|---|
| `native/` | o app: `shared/` (núcleo, captura, encoder, RTP) + `apps/windows` (Slint), `apps/linux` (GTK4), `apps/macos` (SwiftUI); `apps/desktop` é o Tauri, legado | máquina de quem usa | `app` |
| `sfu/` | o relé de mídia (Node 22 + mediasoup) | VPS | `sfu` |
| `web/` | site, contas, servidores, canais, chat, auditoria (Laravel 13 + Livewire 4 + Flux) | VPS | `web` |

- Tarefa de um módulo vai para o agente dele (`.claude/agents/`); a que atravessa os três começa
  pelo contrato.
- **O contrato é [docs/CONTRATO.md](docs/CONTRATO.md)** (API, token de voz, ações e eventos do
  SFU, webhook, tempo real). Mudou o que atravessa a rede: atualize-o na mesma tarefa.
- O app manda a mídia por RTP puro + SRTP no PlainTransport do mediasoup, não por WebRTC.
- Quando o contrato muda, **o SFU sobe antes do app**.

## Os dois modos

- **Sala por código (sem conta):** nome, "criar uma sala", código de 12 caracteres. O código
  **é** a sala: não existe em banco e some com o último. Sem login, sem Laravel no caminho. É
  produto, não legado: não degrade ao mexer no outro modo.
- **Servidores (com conta):** cargos com bits de permissão, canais de texto e voz, sobrescritas
  por cargo e por membro (é assim que se oculta canal), convite, expulsar, banir, chat, voz,
  câmera, tela só de dentro da voz. O Laravel decide e assina um token de 60 s; o SFU só confere
  a assinatura; o app só esconde botão.

| Peça | Dona de | Nunca faz |
|---|---|---|
| Laravel | conta, servidor, cargo, canal, membro, mensagem, auditoria, autorização | mídia |
| SFU | `Room`, `Peer`, producers, consumers, mídia | decidir permissão |
| App | interface, captura, encoder, mídia local | decidir permissão |

As regras de negócio completas estão no `docs/CONTRATO.md` e no agente de cada módulo.

## Rodar local

```bash
cd web && composer dev                                    # Laravel :8000
cd web && php artisan storage:bucket                      # o bucket do MinIO, se quiser antes do primeiro envio
cd sfu && pnpm run build && SFU_SECRET=<o do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js
cd native && UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid-windows   # app do Windows
cd native && UNKVOID_SERVER=http://127.0.0.1:8000 UNKVOID_CAPTURE=x11 cargo run -p unkvoid-linux   # Linux (x11 no WSLg)
cd native/apps/macos && ./run.sh                          # macOS; `./run.sh test`, `./run.sh app`
cd native && cargo run -p core-app --example room -- <ws(s)://…/sfu> <sala> watch|share   # mede fps, perda e pausas
cd native/apps/desktop && VITE_SERVER=http://127.0.0.1:8000 npm run dev:app      # Tauri (legado)
```

Duas máquinas na rede: o IP no lugar de `127.0.0.1` em `APP_URL` e `SFU_PUBLIC_URL`, o SFU com
`SFU_HOST=0.0.0.0 SFU_ANNOUNCED_ADDRESS=<IP>`, o Laravel com `--host=0.0.0.0`; no WSL2,
`networkingMode=mirrored` no `.wslconfig`.

## Verificar antes de entregar

```bash
cd web && composer check            # phpstan max + pint + rector + pest; rode 2x (o rector tem de ficar estável)
cd sfu && pnpm run check && pnpm run build                 # eslint + tsc
cd native && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace   # Linux
cd native && cargo clippy --workspace --exclude unkvoid-linux --all-targets -- -D warnings && cargo test --workspace --exclude unkvoid-linux   # Windows (o GTK não compila lá)
cd native/apps/macos && ./run.sh test
cd native/apps/desktop && npm run check && npm run build   # Tauri (legado); `npm run test:integration` com a pilha no ar
```

- O `opusic-sys` precisa de `cmake` no PATH (Linux e Windows); sem ele o clippy morre com uma
  mensagem que não diz isso.
- Código no WSL, Rust no Windows: [docs/BUILD-WINDOWS.md](docs/BUILD-WINDOWS.md).

## Publicar

App nativo: [docs/AUTO-UPDATE.md](docs/AUTO-UPDATE.md) (Windows, macOS, Linux) e
`docs/BUILD-*.md`. O `release.yml` e o `build-windows.ps1` ainda compilam o Tauri: não solte tag
por eles, senão o Tauri volta por cima do nativo. Contrato mudou: o SFU primeiro.

## Como escrever código

- **Código 100% em inglês**: pastas, classes, métodos, variáveis, colunas, env. Português só em
  caminho de rota, texto de interface e comentário. O
  `native/apps/desktop/tests/static/check-language.py` varre o repo e falha.
- **Comentário explica o porquê**, nunca o quê, e só onde o nome não dá conta. Nunca acima de
  variável.
- **Nada de abreviar variável**: `$exception`, não `$e`.
- **PHP:** `declare(strict_types=1)`, classes `final`, imports no topo, early return,
  `is_null()`, `in_array(..., true)`; escrita em `DB::transaction` + try/catch +
  `Log::channel(...)` com mensagem fixa prefixada por `[ERRO]` e contexto em array. Rota →
  FormRequest → controller → Resource; status de erro mora na exceção. **Um controller por
  recurso** com os métodos dele (`index`, `show`, `store`, `update`, `destroy`…); `__invoke` só
  quando o recurso tem uma ação só. Tela é `Route::view` → blade com `<x-app-layout>` →
  `<livewire:…>`.
- **TypeScript e JavaScript:** uma classe por arquivo, imports no topo, sem comentário
  decorativo. No front do Tauri (`native/apps/desktop/ui`, React + TS estrito) **nenhum
  comentário** — o `tests/static/check-ui.py` falha; os testes dele moram em
  `native/apps/desktop/tests/` (Vitest).
- **Rust:** clippy sem aviso, `cfg(target_os)` certo nas três plataformas, nada de trabalho por
  quadro na thread da captura. **Nunca rode `rustfmt`/`cargo fmt`**: o repo não tem
  `rustfmt.toml`, usa linhas longas, e o formatador quebra dezenas de linhas alheias.
- **Regra de negócio não mora em pasta de sistema:** `native/shared/core` decide, `apps/*`
  desenham. "O Windows vai precisar disto igual?" Se sim, sobe para o `core`.
- **Erro que a pessoa lê nunca tem caminho, URL nem código de status.** O núcleo devolve um
  motivo (`shared/core/src/failure.rs`), cada interface escreve a frase em português, o detalhe
  vai para o log. Exceção: validação, que o Laravel já manda em português sobre o campo.
- Atalho deliberado ganha comentário `ponytail:` com o teto e o caminho de saída.
- **Teste novo entra no arquivo do assunto** (um por área: conta, servidores, mensagens, voz…).
  Nenhum teste some numa reorganização: a contagem de antes cabe na de depois.

## O que precisa do dono

- **Migration:** nunca por iniciativa própria — o esquema é decisão do dono. Pergunte.
- **Regra de negócio** nunca muda de passagem num refactor. Pergunte.
- **Nunca commite, empurre ou abra PR sem autorização explícita naquele momento**, e nunca com
  linha de co-autor ou "Generated with".
- Trabalho grande vira fases; cada fase para para revisão antes do commit.

## Onde está o resto

[docs/README.md](docs/README.md) é o índice: um arquivo por pergunta, do mapa da arquitetura
ao build de cada sistema, mais o `CONTRIBUTING.md` e os checklists do que já foi validado à mão.
