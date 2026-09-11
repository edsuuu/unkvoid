# Para o agente no Mac — build 0.0.8 do macOS

Escrito em 11/09/2026 para quem pegar isto no Mac. O contexto está no
[ROADMAP.md](ROADMAP.md); aqui é só o que precisa acontecer nessa máquina.

## Por que

O app instalado no Mac aponta para `discord.unkvoid.com`, que não existe mais no
DNS: fica em "Sem conexão, reconectando…" para sempre. O site, o SFU, o APT e o
e-mail agora vivem em `unkvoid.com`, e o código desta branch já aponta para lá
(`native/apps/desktop/ui/app.js`, `App.SERVER`, e o `tauri.conf.json`, que também
subiu para 0.0.8 e lê o atualizador em `https://unkvoid.com/downloads/latest.json`).

O Linux 0.0.8 já está publicado. Falta o macOS, e ele só sai de um Mac.

## Antes de começar

1. O código precisa estar na branch `feat/site-laravel` no GitHub. Se `git fetch`
   não trouxer a branch, ela ainda não foi enviada da máquina do WSL: peça.
2. A chave privada do atualizador tem de existir em `~/.tauri/unkvoid.key`. Sem
   ela o `.app.tar.gz` sai sem `.sig`, o instalador funciona, e ninguém se
   atualiza sozinho. A pública está em `plugins.updater.pubkey` do
   `tauri.conf.json`; a conferência de que batem está em [AUTO-UPDATE.md](AUTO-UPDATE.md).
3. `RELEASE_SECRET`: é o segredo com que o build se registra no site. Está em
   `~/auxilos/release-secret.env` na máquina do WSL e no `.env` do site na VPS
   (`/var/www/projects/unkvoid-web/shared/.env`). Copie para o Mac, fora do
   repositório, por exemplo `~/.config/unkvoid/release-secret.env`.

## O que fazer

```bash
git fetch && git checkout feat/site-laravel && git pull
cd native/apps/desktop
npm ci
npm run check

TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/unkvoid.key)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
npx tauri build --bundles app,dmg
```

Confira que saíram, em `native/target/release/bundle/`:

- `macos/Unkvoid.app.tar.gz` **e** `macos/Unkvoid.app.tar.gz.sig` — o artefato do
  atualizador. Sem o `.sig`, pare e volte ao passo 2.
- `dmg/Unkvoid_0.0.8_aarch64.dmg` — o instalador para quem baixa pela primeira vez.

Publique os dois no site. O script assina a chamada com o `RELEASE_SECRET`, o
Laravel guarda no MinIO e monta o `latest.json`:

```bash
set -a; . ~/.config/unkvoid/release-secret.env; set +a
./publish-release.sh darwin-aarch64     ../../target/release/bundle/macos/Unkvoid.app.tar.gz ../../target/release/bundle/macos/Unkvoid.app.tar.gz.sig
./publish-release.sh darwin-aarch64-dmg ../../target/release/bundle/dmg/Unkvoid_0.0.8_aarch64.dmg
```

Depois disso `https://unkvoid.com/downloads/latest.json` passa a ter
`darwin-aarch64`, e o card do macOS no site ganha o botão do `.dmg`.

## Instalar e provar que funciona

```bash
cd ../../..   # raiz do repositório
make install  # gera o .app, troca o de /Applications e abre
```

Na ordem:

1. O app passa da tela de conexão e mostra a entrada. Se ficar em "Servidor sem
   resposta", `curl https://unkvoid.com/health` tem de devolver `{"ok":true,...}`.
2. Crie uma sala, copie o código, entre com outro aparelho (o Linux 0.0.8 serve) e
   compartilhe a tela. Vídeo e áudio dos dois lados.
3. Feche e abra o app: ele consulta o `latest.json`. Como a versão instalada é a
   mesma, não faz nada — é o esperado.

## O que NÃO fazer

- Não mexa em `tauri.conf.json` além do que já está na branch. A versão 0.0.8 é a
  mesma do Linux e do Windows, e é assim que o `latest.json` fecha.
- Não commite a chave nem o `RELEASE_SECRET`.
- Não use `node release.mjs` nem o GitHub Releases: o atualizador não olha mais
  para lá.

## Enquanto o build não existe

O Mac atual volta a funcionar se `discord.unkvoid.com` voltar a resolver: registro
`A discord → 144.126.133.10` na Porkbun, e um `certbot --expand` na VPS para o
certificado cobrir o nome. O nginx já atende por ele. Isso é uma ponte, não a
solução: o build 0.0.8 acima é que resolve.
