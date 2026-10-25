# Atualização automática e publicação

| Sistema | Como a versão nova chega |
|---|---|
| Windows (instalador do site) | o app baixa, confere a assinatura e oferece a atualização |
| Windows (Microsoft Store) | pela Store; o app não procura versão no site |
| Linux | pelo `apt upgrade`; o app não se atualiza sozinho |

## As duas chaves, que são diferentes

| Chave | O que assina | Onde mora |
|---|---|---|
| Atualização (minisign) | o instalador `.exe` do site | `~/auxilos/unkvoid.key`, só com quem publica |
| Repositório APT (GPG) | o índice de pacotes do Linux | `repo@unkvoid.com`, no chaveiro da VPS |

A pública da atualização tem uma fonte única:
[`native/release/update-public-key.pub`](../native/release/update-public-key.pub). O arquivo
guarda o texto minisign em Base64, formato que o manifesto carrega. Tanto
`native/shared/core/src/update.rs` quanto `native/release/check-signature.mjs` leem essa cópia.
O par em uso tem o identificador `9a18c9243ef59b08`.

A privada não vai para a VPS. Quem a tiver pode publicar uma atualização para todo mundo; trocar
a pública também exige uma instalação manual intermediária para quem já instalou o app.

## Onde o app procura

`https://unkvoid.com/downloads/latest.json` é montado pelo Laravel a partir de `releases`, com
URLs do MinIO assinadas por uma hora. Cada plataforma registra apenas sua linha pela API assinada
de `native/release/publish-release.sh`; publicar uma plataforma não apaga as outras.

No Windows, a chave é `windows-x86_64-nsis`: o instalador do site é o `.exe` NSIS. O `.msix` não
entra no manifesto porque a Microsoft Store faz a atualização dele.

## Windows

`native/shared/core/src/update.rs` compara versões semver, baixa o instalador, confere a
assinatura antes de gravá-lo e só então mostra a atualização. O clique fecha a sessão atual e abre
o instalador com `/P /UPDATE /R`; a pessoa escolhe quando trocar de versão.

### Publicar o instalador do site

1. Altere `version` em `[workspace.package]` de `native/Cargo.toml`.
2. No Windows, gere o instalador conforme [BUILD-WINDOWS.md](BUILD-WINDOWS.md).
3. No WSL, instale o assinador uma vez. Em Debian/Ubuntu:

   ```bash
   sudo apt update
   sudo apt install minisign
   ```

4. Ainda no WSL, assine, confira e publique. `sign-update.sh` produz a mesma string minisign em
   Base64 que o app já verifica; não use o `.sig` textual que o `minisign` gera diretamente. Ele
   aceita tanto o arquivo privado bruto do `minisign` quanto a chave legada criada por `tauri
   signer generate`, que encapsula esse arquivo em Base64.

   ```bash
   VERSION=0.2.0
   EXE=/mnt/c/Users/edsu/unkvoid-app-rust/native/target/release/bundle/windows/Unkvoid_${VERSION}_x64-setup.exe
   export UNKVOID_UPDATE_SECRET_KEY="$HOME/auxilos/unkvoid.key"

   native/release/sign-update.sh "$EXE"
   node native/release/check-signature.mjs "$EXE.sig"
   RELEASE_SECRET="$(cut -d= -f2- "$HOME/auxilos/release-secret.env")" \
       native/release/publish-release.sh windows-x86_64-nsis "$EXE" "$EXE.sig"
   ```

Na passagem de `0.1.19` para `0.2.0`, o executável continua instalado como
`unkvoid-desktop.exe`, no mesmo diretório e com o identificador `com.unkvoid.desktop`; o novo
binário do Cargo é renomeado no instalador. O app mantém `%APPDATA%\com.unkvoid.desktop`, então
login e preferências continuam disponíveis. O instalador aceita `/P /UPDATE /R`: instalação
passiva, sem recriar o atalho da área de trabalho já removido pela pessoa, e reabre o app.
O beta usa outro AppUserModelID e outra pasta de preferências.

`publish-release.sh` lê a versão do `[workspace.package]` automaticamente. Use
`RELEASE_VERSION` apenas para registrar deliberadamente um instalador antigo.

Quando a chave for cifrada, o `minisign` pede a senha no terminal. Não a inclua na linha de
comando nem a grave no repositório.

O workflow pausado `release.yml` instala `minisign` e usa o segredo
`UNKVOID_UPDATE_SECRET_KEY` com o mesmo conteúdo da chave privada. Antes de reativá-lo, ele
precisa de uma forma segura de fornecer a senha da chave cifrada ao runner sem gravá-la no
repositório. Ele não instala nem executa o aplicativo antigo.

## Linux

O app único, ainda hospedado provisoriamente em `native/apps/unkvoid`, gera o `.deb` no Debian
12. O pacote vai ao APT pelo publicador em `native/release/`:

```bash
native/apps/unkvoid/build-deb.sh
scp native/target/deb12/Unkvoid_0.2.0_amd64.deb vps:/tmp/
ssh vps 'cd /var/www/projects/unkvoid/native/release && ./apt-publish.sh /tmp/Unkvoid_0.2.0_amd64.deb'
```

`native/release/validate-deb.sh <arquivo.deb>` instala o arquivo em uma Debian 12 limpa antes da
publicação. O `.deb` não recebe assinatura minisign própria; o APT autentica o índice GPG.

Para instalar, uma vez só:

```bash
curl -fsSL https://unkvoid.com/apt/unkvoid.gpg | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] https://unkvoid.com/apt ./" | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

## Quando algo não atualiza

1. `curl -s https://unkvoid.com/downloads/latest.json`: 404 significa que não há instalador
   assinado registrado.
2. A `version` do manifesto é maior que a instalada?
3. Há `windows-x86_64-nsis` no manifesto para quem instalou pelo `.exe`?
4. `node native/release/check-signature.mjs <arquivo.sig>` reconhece a chave?
5. A URL do instalador responde 200? Releia o manifesto se a URL assinada já venceu.
6. No Linux, a atualização é `apt update && apt upgrade`; na Store, é a própria Store.
