# Unkvoid — estado do projeto

> **07/09/2026.** O projeto mudou de forma. Deixou de ser um clone de Discord e virou
> uma coisa só: **compartilhar tela com quem você mandar o código**. Sem conta, sem
> servidor, sem canal, sem chat, sem microfone. Este documento descreve o que o projeto
> **é agora**. O [ARQUITETURA.md](ARQUITETURA.md) ainda descreve o modelo antigo.

**Repo:** github.com/edsuuu/unkvoid (privado) · **VPS:** 144.126.133.10 (Contabo, EUA)
**Domínio:** discord.unkvoid.com

---

## O app, em uma frase

Você baixa, escreve seu nome, clica em **Criar uma sala**, e recebe um código de 12
caracteres. Manda o código para quem quiser. Quem cola o código entra e vê a tela de
quem estiver transmitindo — em grade, em foco, ou em tela cheia.

O código **é** a sala. Não existe em banco nenhum. Quando o último sai, ela acaba.

---

## O que está pronto e verificado

- Entrada por nome + código, com o nome lembrado entre aberturas
- `POST /api/rooms`: sorteia o código e emite o token do SFU (9 testes)
- Sala: grade (colunas pela raiz do total), **Focar** por quadro, **Tela cheia**
- Seletor do que transmitir, com abas Telas/Aplicativos e miniatura de cada item
- Transmissão sempre pelo SFU, então **a web também vê** o que o app manda
- Instalação, auto-atualização (na abertura e a cada 6h, com progresso), bandeja,
  início com o sistema

**Não verificado com duas máquinas de verdade.** Os checks e o harness cobrem a lógica;
app-transmitindo-e-alguém-assistindo depende de deploy e de duas pessoas.

---

## O que seria necessário de verdade — a conta

O app inteiro chama **dois endpoints**: `/api/health`, que responde `{"ok":true}`, e
`/api/rooms`. Para servir esses dois existem hoje **112 arquivos e ~7.900 linhas** de
PHP/JS, mais MySQL, Reverb e PHP-FPM.

O SFU já sobe um servidor HTTP próprio — é ele que faz o upgrade para o WebSocket.
Colocar `/rooms` e `/health` ali são poucas linhas.

### Necessário

| | Por quê |
|---|---|
| **SFU** (Node + mediasoup) | insubstituível: um transmissor vira N espectadores sem multiplicar o upload |
| **nginx** | só termina TLS para o `wss://`. Já existe, não muda |
| **O app** | — |
| **GitHub Releases** | já hospeda instaladores e `latest.json`. Não precisa de servidor |

Banco de dados **já é zero**. Não é que dê para tirar: já não tem.

### Descartável

**Laravel inteiro** — 112 arquivos, ~7.900 linhas, MySQL e Reverb. Some o PHP da VPS.

E dentro do próprio SFU, ~350 linhas que só existiam para o modelo Discord:

| Peça | Linhas | Por quê morre |
|---|---|---|
| `PresenceRegistry` | 180 | presença entre canais de um servidor. Não há servidor nem canal |
| `ModerationController` | 53 | dono, expulsar, forçar parar. Não há conta nem dono |
| `StateController` | 28 | mudo/surdo. Não há microfone |
| `SignalController` | 26 | relé do P2P, já sem chamador |
| `TokenVerifier` | 40 | ver abaixo |

### O token hoje não é autenticação

Com sala anônima, o token não prova quem você é — **prova que você passou pelo endpoint
que tem limite de taxa**. É um recibo de rate limit. O SFU pode aplicar esse limite por
IP direto, e aí o token e o `SFU_SECRET` compartilhado entre dois processos somem.

O que não muda: **um serviço de sala anônima é um relé aberto por construção.** Quem
descobrir o endereço cria sala e empurra tráfego pela VPS. Isso já vale hoje — o
`/api/rooms` é público. A defesa é limite de taxa e teto de banda, não token. Tirar o
Laravel não piora nada; só para de fingir que protege.

---

## Decisão em aberto: o espectador pelo navegador

Derrubar o web tem uma consequência que vale encarar: **"manda o código pro amigo" fica
muito mais fraco se o amigo precisar instalar o app primeiro.** Hoje ele abre o
navegador e assiste.

**App-only.** Mais limpo, some tudo. Quem quiser ver, instala.

**Página estática de espectador, servida pelo próprio SFU.** Uma HTML só: cola o código,
vê os quadros em grade/foco/tela cheia. Sem Livewire, conta, banco ou PHP. Reaproveita o
`SfuClient.js`, que já existe e funciona. ~200 linhas, e o Laravel morre igual.

**Recomendação: a segunda.** O custo é uma página; o ganho é que qualquer um com o link
assiste, inclusive de celular — que é o que o código de sala promete.

---

## Pendências

### Segurança, e é a única urgente

`harness.html` teve um **Bearer Sanctum válido de produção** hard-coded. Ele saiu do
arquivo, mas continua em todos os commits antigos — e **o repositório esteve público**
por um tempo com ele lá dentro. Voltar a privado não desfaz isso: pode já ter sido
raspado. Precisa ser revogado:

```bash
ssh ubuntu@144.126.133.10 "cd /var/www/projects/discord/current && php artisan tinker --execute='Laravel\Sanctum\PersonalAccessToken::find(6)?->delete();'"
```

### Deploy parado

`sfu/` e `web/` mudaram e **não foram para a VPS**. O endpoint `/api/rooms` só existe
aqui; sem o deploy, o app instalado não cria sala nenhuma contra produção.

```bash
ssh ubuntu@144.126.133.10
cd /var/www/projects/discord/current && git pull
cd /var/www/projects/sfu && git pull && npm run build && pm2 restart sfu
```

### Faltando no Windows

- **Encoder Media Foundation.** A captura já pega os quadros certos e respeita a tela
  escolhida, mas `on_frame_arrived` descarta os pixels (`surface: None`). Sem encoder,
  `start_broadcast` falha — e agora a falha aparece na tela em vez de sumir calada.
- Miniatura no seletor devolve vazio fora do macOS.

O código Windows **nunca foi executado**, em lugar nenhum.

### Limpeza que sobrou

- `PeerLink` e companhia no Rust (`crates/media/src/peer.rs`, `offer_to`,
  `accept_answer`, `add_candidate`, `drop_viewer`) ficaram sem chamador quando o app
  passou a transmitir sempre pelo SFU. É deleção pura.
- `tauri-plugin-deep-link` e o esquema `discord2://` ficaram sem uso: eram do login
  Google.

---

## CI

**Desligado de propósito** (`gh workflow disable desktop.yml`). Reativa com
`gh workflow enable desktop.yml`.

Com o repositório privado o minuto é cobrado, e com multiplicador: **macOS 10x, Windows
2x**. Uma release completa custava ~$15, sendo ~$14.40 só de macOS. Os 3.000 minutos
inclusos de setembro acabaram em um dia de trabalho — não houve cobrança, o GitHub para
em vez de cobrar quando o limite de gasto é $0.

Já cortado: o `verify` roda **só no Linux** em push (era ~39 minutos faturados por
commit, virou ~3). Windows e macOS continuam sendo verificados na tag e no dispatch.

---

## Como gerar build

### macOS (nesta máquina)

```bash
cd native/apps/desktop && npx tauri build --bundles app
cp -R ../../target/release/bundle/macos/Unkvoid.app /Applications/
```

### Windows

```bash
cd unkvoid\native\apps\desktop
npm ci
npx tauri build
```

Sai em `native\target\release\bundle\msi\Unkvoid_0.0.2_x64_en-US.msi`. Precisa antes de
**Rust** (rustup, toolchain MSVC), **Node 22** e **Visual Studio Build Tools** com a
carga "Desenvolvimento para desktop com C++". O WiX o Tauri baixa sozinho.

Para o `.msi` sair assinado — sem assinatura ninguém se atualiza sozinho:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $HOME\.tauri\unkvoid.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
```

A chave privada está em `~/.tauri/unkvoid.key` **no Mac** e precisa ser levada para lá.
A pública já está no `tauri.conf.json`. A antiga foi trocada porque só existia no secret
do GitHub, que não se lê de volta.

### Publicar

```bash
node release.mjs --dry-run    # confere o que achou
node release.mjs              # cria/atualiza a release e o latest.json
```

Instalador **não cross-compila**: `.msi` só sai no Windows, `.dmg` só no macOS. O
`release.mjs` lê o `latest.json` já publicado e mescla, então subir o Windows depois do
macOS não deixa os Macs sem para onde atualizar.

---

## Armadilhas já pagas

- **`npm run check` antes de qualquer commit no desktop.** Três vezes um script de
  substituição em bloco apagou um método inteiro do `app.js`. O sintoma é tela preta ou
  clique que não faz nada.
- **`hidden` do Tailwind é classe, não atributo.** Alternar o atributo num elemento que
  tem a classe não faz nada. Custou dois bugs — o botão "Parar de compartilhar" que
  nunca aparecia foi um deles.
- **Teste no `harness.html`, não na janela do app.** A janela do Tauri não tem console:
  um erro de JS vira tela preta sem pista. O harness roda o mesmo bundle no navegador,
  com a ponte do Tauri e o servidor fingidos.
- **`build.rs` tem `cargo:rerun-if-changed=../dist`.** Sem isso o cargo não recompila
  quando só o frontend muda, e o app sai com a interface da última vez que o Rust mudou.
  Não remova.
- **`use_sfu` depois de declarar vídeo E áudio.** Ao contrário, o Rust manda RTP de um
  SSRC que o servidor ainda não conhece e ele descarta calado: a transmissão "funciona"
  e ninguém vê nada. `check-broadcast.mjs` guarda essa ordem.
- **Release não pode ser pré-lançamento.** O endpoint do auto-update é
  `/releases/latest/download/latest.json`, e o "latest" do GitHub **ignora**
  pré-lançamentos. Com todas marcadas assim, a URL responde 404 — foi o que fez a v0.2.0
  até a v0.7.0 nunca atualizarem ninguém.
