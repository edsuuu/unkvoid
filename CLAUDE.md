# Unkvoid

Compartilhar a tela com quem você mandar o código, **sem perder fps no jogo** — e, para quem
tem conta, servidores com canais de texto e voz, câmera e chat, no molde do Discord.

O objetivo do parágrafo acima manda em todo o resto. No navegador o encoder de vídeo roda na
CPU, então o jogo e a compressão disputam o mesmo processador e a transmissão cai para 1 fps. O
app existe para usar o chip de codificação da placa de vídeo:
`captura → textura na GPU → encoder de hardware → 1 quadro → SFU → N espectadores`.
O quadro não desce para a memória do processador antes de ser comprimido, é comprimido **uma
vez** e sobe **uma vez**. Mudança que quebre uma dessas três coisas está desfazendo o projeto.

## As três peças

| Pasta | O quê | Onde roda | Agente |
|---|---|---|---|
| `native/` | o app: captura, encoder, interface (Rust + Tauri + React em TypeScript) | máquina de quem usa | `app` |
| `sfu/` | o relé de mídia (Node 22 + mediasoup) | VPS | `sfu` |
| `web/` | site, contas, servidores, canais, chat, auditoria (Laravel 13 + Livewire 4 + Flux) | VPS | `web` |

Há um agente especialista por módulo em `.claude/agents/`. Tarefa que toca um módulo só vai
para o agente dele; tarefa que atravessa os três começa pelo contrato.

**O contrato é [docs/SERVIDORES.md](docs/SERVIDORES.md)**: rotas da API, formato do token de voz, ações e
eventos do SFU, webhook, canais do Reverb, comandos do Tauri. Mudou o que atravessa a rede,
atualize esse arquivo na mesma tarefa — as três peças o leem como lei.

## Os dois modos, e por que os dois existem

**Sala por código (sem conta).** Nome, "criar uma sala", um código de 12 caracteres. O código
**é** a sala: não existe em banco nenhum e some quando o último sai. Nada de login, nada de
Laravel no caminho. Isso é produto, não legado: não degrade esse caminho ao mexer no outro.

**Servidores (com conta).** Servidor, cargos com bits de permissão, canais de texto e voz,
sobrescritas por cargo e por membro (é assim que se oculta canal), convite, expulsar, banir,
chat em tempo real, voz, câmera, e tela só de dentro da voz. O Laravel decide quem pode o quê e
assina um token de 60 s; o SFU só confere a assinatura; o app só esconde botão.

Quem manda em quê:

| Peça | Dona de | Nunca faz |
|---|---|---|
| Laravel | conta, servidor, cargo, canal, membro, mensagem, auditoria, autorização | mídia |
| SFU | `Room`, `Peer`, producers, consumers, mídia | decidir permissão |
| App | interface, captura, encoder, mídia local | decidir permissão |

As regras de negócio completas (cálculo de permissão efetiva na ordem do Discord, hierarquia de
cargos, canal oculto, voz, auditoria) estão em `docs/SERVIDORES.md` e no agente de cada módulo.

## Rodar tudo local

```bash
# Laravel (API, site, painel) + Reverb (chat e presença)
cd web && composer dev                 # :8000
cd web && php artisan reverb:start     # :8080

# SFU — o segredo tem de ser o mesmo SFU_SECRET do web/.env
cd sfu && pnpm run build
cd sfu && SFU_SECRET=<o do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js

# O bucket do MinIO (AWS_BUCKET): o primeiro clipe e a primeira versão publicada o criam
# sozinhos; para criar antes, à mão
cd web && php artisan storage:bucket

# App apontando para o Laravel local (SFU e Reverb vêm do GET /api/config)
cd native/apps/desktop && VITE_SERVER=http://127.0.0.1:8000 npm run dev:app

# Só a interface, num navegador comum: o Vite faz o papel do nginx e a ponte do Tauri é fingida
cd native/apps/desktop && VITE_SERVER=http://localhost:1420 npm run dev
```

Duas máquinas na mesma rede: troque `127.0.0.1` pelo IP em `APP_URL`, `SFU_PUBLIC_URL` e
`REVERB_HOST` (`web/.env`), suba o SFU com `SFU_HOST=0.0.0.0 SFU_ANNOUNCED_ADDRESS=<IP>` e o
Laravel com `--host=0.0.0.0`. No WSL2 a rede só alcança o UDP do SFU com
`networkingMode=mirrored` no `.wslconfig`.

## Verificar antes de entregar

```bash
cd web && composer check                 # phpstan max + pint + rector + pest em SQLite (rode 2x: rector estável)
cd web && composer test:mysql            # a mesma suíte no MySQL: pega tipo de coluna e chave que o SQLite perdoa
cd sfu && pnpm run check                 # eslint + check.mjs + check-heartbeat.mjs (precisa de um servidor no ar com o mesmo SFU_SECRET; o cenário de clipe só roda no Linux, porque o anel chama o ffmpeg por `setpriv`)
cd native/apps/desktop && npm run check && npm run build   # tests/static + tsc + eslint + Vitest (tests/unit)
cd native/apps/desktop && npm run test:integration    # Vitest: os clientes do app contra a pilha local no ar
cd native && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Windows não compila de dentro do WSL: espelhe `native/` em `/mnt/c/Users/edsu/unkvoid-build/` com
`rsync` e use `/mnt/c/Users/edsu/.cargo/bin/cargo.exe`. O instalador sai com
`cmd.exe /c "npm.cmd ci && npx.cmd tauri build --bundles nsis"` (o PowerShell desta máquina
bloqueia `npx.ps1`) e termina reclamando de `TAURI_SIGNING_PRIVATE_KEY` — o `.exe` já está pronto
em `native/target/release/bundle/nsis/`.

## Como escrever código aqui

Vale a skill `style-edsu` inteira, e o resumo que mais pega:

- **Código 100% em inglês** — pastas, classes, métodos, variáveis, colunas, env. Português só em
  caminho de rota, texto de interface e comentário. Há um check no repo
  (`native/apps/desktop/tests/static/check-language.py`) que varre tudo e falha.
- **Comentário explica o porquê**, nunca o quê, e só onde o nome não dá conta. Nunca acima de
  variável.
- **Nada de abreviar variável**: `$exception`, não `$e`.
- PHP: `declare(strict_types=1)`, classes `final`, imports no topo, early return, `is_null()`,
  `in_array(..., true)`, escrita em `DB::transaction` + try/catch + `Log::channel(...)` com
  mensagem fixa prefixada por `[ERRO]` e contexto em array. Rota → FormRequest → controller →
  Resource; status de erro mora na exceção. **Um controller por recurso**, com os métodos dele
  (`index`, `show`, `store`, `update`, `destroy` e o que mais o recurso tiver) — controller de
  ação única só quando o recurso tem uma ação só de verdade, e aí com `__invoke`. Tela é
  `Route::view` → blade com `<x-app-layout>` → `<livewire:…>`.
- TypeScript e JavaScript: uma classe por arquivo, imports no topo, sem comentário decorativo.
- Front do app (`native/apps/desktop/ui`, React + TypeScript estrito): **nenhum comentário**, o nome
  explica — decisão do dono, e o `tests/static/check-ui.py` falha com comentário lá. Os testes do
  app moram em `native/apps/desktop/tests/` (Vitest).
- Rust: clippy sem aviso, `cfg(target_os)` correto nas três plataformas, nada de trabalho por
  quadro na thread da captura.
- Atalho deliberado ganha comentário `ponytail:` com o teto e o caminho de saída.
- **Migration nunca é criada por iniciativa própria** — o esquema é decisão do dono. Pergunte.
- **Regra de negócio nunca muda de passagem** num refactor. Pergunte.
- **Nunca commite, empurre ou abra PR sem autorização explícita naquele momento**, e nunca com
  linha de co-autor ou "Generated with". Trabalho grande vira fases, e cada fase para para
  revisão antes do commit.

## Onde está escrito o resto

| Arquivo | Para quê |
|---|---|
| [docs/SERVIDORES.md](docs/SERVIDORES.md) | o contrato entre as três peças, e como rodar local |
| [docs/ESTADO.md](docs/ESTADO.md) | o que só foi escrito sem rodar em hardware, o que falta e as perguntas abertas |
| [docs/DECISOES.md](docs/DECISOES.md) | o que foi decidido e **por quê** (ex.: por que o SFU é Node) |
| [docs/REDE.md](docs/REDE.md), [docs/UDP.md](docs/UDP.md) | portas, firewall, o que vai por UDP e por quê |
| [docs/SERVIDOR.md](docs/SERVIDOR.md) | a VPS que existe: medições, o que cada número resolveu, o que desligar |
| [docs/INSTALAR-VPS.md](docs/INSTALAR-VPS.md) | levantar uma VPS do zero, em ordem de execução, e migrar o e-mail |
| [docs/SEGURANCA.md](docs/SEGURANCA.md) | modelo de ameaça e o que protege o quê |
| [docs/AUTO-UPDATE.md](docs/AUTO-UPDATE.md), [docs/BUILD-WINDOWS.md](docs/BUILD-WINDOWS.md), [docs/BUILD-MACOS.md](docs/BUILD-MACOS.md), [docs/BUILD-LINUX.md](docs/BUILD-LINUX.md) | publicar e buildar por sistema |
| `web/tests/checklist.html` | o que já foi validado à mão |
