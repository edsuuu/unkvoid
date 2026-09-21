# Contribuindo com o Unkvoid

Obrigado por querer mexer aqui. Este arquivo é o caminho curto: o que ler, como subir o
projeto, o que rodar antes de abrir um PR e o que precisa de conversa antes de virar código.
Participar do projeto é aceitar o [Código de Conduta](CODE_OF_CONDUCT.md).

## Leia antes de escrever

1. [README.md](README.md): o que o projeto é e por que é um app.
2. [docs/ARQUITETURA.md](docs/ARQUITETURA.md): as três peças, como conversam, os fluxos.
3. [docs/CONTRATO.md](docs/CONTRATO.md): tudo o que atravessa a rede. **É a lei das três peças.**
4. [docs/ESTADO.md](docs/ESTADO.md): o que falta, o que nunca rodou em hardware e as perguntas
   que esperam o dono — o melhor lugar para achar o que fazer.

Uma regra manda em todas as outras: **compartilhar a tela sem perder fps no jogo**. O quadro não
desce para a memória do processador antes de ser comprimido, é comprimido uma vez e sobe uma
vez. Mudança que quebre uma dessas três coisas não entra, por melhor que seja o resto.

## As três peças

| Pasta | O quê | Pilha |
|---|---|---|
| `native/` | o app: captura, encoder, interface | Rust, Tauri 2, React 19 em TypeScript estrito |
| `sfu/` | o relé de mídia | Node 22, mediasoup, pnpm |
| `web/` | site, contas, servidores, chat, auditoria | Laravel 13, Livewire 4, Flux, Pest |

Mudança que toca uma peça só fica dentro da pasta dela. Mudança que atravessa a rede (rota,
evento, token, comando do Tauri) **começa pelo contrato**: primeiro o `docs/CONTRATO.md`, depois
o código das peças.

## Ambiente

- **Rust** (rustup, toolchain padrão) e **cmake** (o Opus compila com ele).
- **Node 22+**, `npm` para o app e `pnpm` para o SFU.
- **PHP 8.4** e **Composer** para o Laravel.
- **Docker** para MySQL, MinIO e e-mail de teste: `infra/docker-compose.yml` é o da VPS; local,
  bastam um MySQL em `127.0.0.1:3306` (`root`/`root`) e um MinIO em `127.0.0.1:9000`, que é o
  que o `web/.env.example` espera.
- **Linux:** `libwebkit2gtk-4.1-dev libgtk-3-dev build-essential`, `gstreamer1.0-tools` e os
  plugins good/bad/ugly.
- **Windows:** Visual Studio Build Tools com "Desenvolvimento para desktop com C++". Não compila
  de dentro do WSL: veja [docs/BUILD-WINDOWS.md](docs/BUILD-WINDOWS.md).
- **macOS:** Xcode Command Line Tools; veja [docs/BUILD-MACOS.md](docs/BUILD-MACOS.md).

## Rodar tudo local

```bash
# Laravel: API, site e o resto do que precisa de banco
cd web && composer setup               # primeira vez: .env, chave, dependências
cd web && composer dev                 # :8000

# SFU — o segredo tem de ser o mesmo SFU_SECRET do web/.env (32 caracteres ou mais)
cd sfu && pnpm install && pnpm run build
cd sfu && SFU_SECRET=<o do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js

# App apontando para o Laravel local (a URL do SFU vem do GET /api/config)
cd native/apps/desktop && npm ci
cd native/apps/desktop && VITE_SERVER=http://127.0.0.1:8000 npm run dev:app

# Só a interface, num navegador comum, com a ponte do Tauri fingida
cd native/apps/desktop && VITE_SERVER=http://localhost:1420 npm run dev
```

Duas máquinas na mesma rede, WSL2 e o resto dos detalhes: fim do
[docs/CONTRATO.md](docs/CONTRATO.md#rodar-tudo-local-para-testar-antes-de-subir).

## Antes de abrir o PR

Rode o que corresponde à peça em que você mexeu. PR só entra verde: o merge na `main` faz o
deploy do site e do SFU sozinho e publica o `.deb` do Linux.

```bash
cd web && composer check          # phpstan max + pint + rector + pest em SQLite (rode 2x: o rector tem de ficar estável)

cd sfu && pnpm run check          # eslint + check.mjs (precisa de um servidor no ar com o mesmo SFU_SECRET)

cd native/apps/desktop && npm run check && npm run build   # checks estáticos + tsc + eslint + Vitest
cd native && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Comportamento novo vem com teste: Pest (Feature, nome em frase, e o negativo de autorização é
obrigatório) no `web/`, cenário no `check.mjs` do `sfu/`, Vitest em `tests/unit` e teste
unitário em Rust no `native/`. Prefira acrescentar no arquivo de teste do assunto a criar um
arquivo novo por função.

O que só dá para provar em hardware (placa de vídeo, Wayland, um Mac) e você não tem à mão:
escreva, e registre em `docs/ESTADO.md` na seção "Escrito, mas nunca rodou em hardware". É
melhor um item honesto ali do que um "funciona" que ninguém viu.

## Como escrever código aqui

- **Código 100% em inglês**: pastas, classes, métodos, variáveis, colunas, env. Português só em
  caminho de rota, texto de interface e comentário. `native/apps/desktop/tests/static/check-language.py`
  varre o repositório e falha.
- **Comentário explica o porquê**, nunca o quê, e só onde o nome não dá conta. Na interface do
  app (`native/apps/desktop/ui`) não há comentário nenhum: o nome explica, e o `check-ui.py` cobra.
- **Nada de abreviar variável**: `$exception`, não `$e`.
- **PHP:** `declare(strict_types=1)`, classes `final`, early return, rota → FormRequest →
  controller → Resource, um controller por recurso, autorização no modelo, escrita em
  transação com log `[ERRO]`.
- **TypeScript:** estrito, uma classe por arquivo em `ui/core`, um componente por arquivo, regra
  de negócio nunca em componente.
- **Rust:** clippy sem aviso, `cfg(target_os)` correto nos três sistemas, nada de trabalho por
  quadro na thread da captura.
- **Atalho deliberado** ganha comentário `ponytail:` dizendo o teto e o caminho de saída.
- O mínimo que resolve. Dependência nova só quando algumas linhas não dão conta.

## O que precisa de conversa antes

Abra uma issue (ou pergunte no PR) **antes** de escrever quando a mudança for:

- **migration** — o esquema do banco é decisão do dono do projeto;
- **regra de negócio** — quem pode o quê, limites, o que acontece ao expulsar, e parecidos;
- **contrato** — formato de rota, evento, token ou comando do Tauri que as outras peças leem.

## Fluxo

A `main` é protegida no GitHub: só entra por pull request, e não aceita force-push nem ser
apagada. Vale para todo mundo, inclusive para quem mantém o projeto.

1. Branch a partir da `main`, com nome que diz o que é (`volta-sala-sem-login`,
   `fix/login-google-windows`).
2. Commits em português, no formato `tipo: o que mudou` (`feat`, `fix`, `docs`, `chore`), sem
   linha de co-autor.
3. PR para a `main` dizendo o que mudou, como foi validado e o que **não** foi validado.
4. Mudou o que atravessa a rede, o `docs/CONTRATO.md` muda no mesmo PR. Fechou ou abriu uma
   pendência, o `docs/ESTADO.md` também.

## Armadilhas já pagas

- **`npm run check` antes de qualquer commit no app.** Interface quebrada vira tela preta sem
  pista: a janela do Tauri não tem console. `npm run dev` roda o mesmo código no navegador, com
  console; o `harness.html` abre um build pronto numa máquina que não compila o Rust.
- **`use_sfu` só depois de declarar vídeo E áudio.** Ao contrário, o Rust manda RTP de um SSRC
  que o servidor ainda não conhece e ele descarta calado: a transmissão "funciona" e ninguém vê
  nada. O `tests/unit/broadcast.test.ts` guarda essa ordem.
- **`hidden` do Tailwind é classe, não atributo.** Alternar o atributo num elemento que tem a
  classe não faz nada.
- **`build.rs` tem `cargo:rerun-if-changed=../dist`.** Sem isso o cargo não recompila quando só
  o frontend muda, e o app sai com a interface antiga. Não remova.
- **O crate `capture` tem um módulo chamado `windows`.** Dentro dele, `windows::Win32::…` acha o
  módulo local em vez da crate da Microsoft. Precisa de `::windows::`.
- **Ponteiro COM não é `Send`.** O encoder atravessa uma vez para a thread da captura, e há um
  `unsafe impl Send` com a justificativa escrita: os objetos do D3D11 (com proteção multithread
  ligada) e o MFT assíncrono são livres de apartamento.
- **Porta UDP fechada não dá erro.** A transmissão "funciona" e ninguém vê nada:
  [docs/UDP.md](docs/UDP.md).

## Segurança

Achou uma falha? Não abra issue pública: escreva para **contato@unkvoid.com**. O que o projeto
protege e o que não protege está em [docs/SEGURANCA.md](docs/SEGURANCA.md).

## Licença

O projeto é [MIT](LICENSE). Ao contribuir, você concorda que a sua contribuição sai sob a mesma
licença.
