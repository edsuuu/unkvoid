# Atualização automática e publicação

| Sistema | Como a versão nova chega |
|---|---|
| Windows (instalador do site) | o app baixa, confere a assinatura e mostra o botão verde de atualizar |
| Windows (Microsoft Store) | pela Store; o app não procura nada no site |
| Linux | pelo `apt upgrade`; o app não se atualiza sozinho, de propósito |
| macOS | ainda sem versão publicada |

## As duas chaves, que são diferentes

Confundir uma com a outra é o erro mais caro deste arquivo.

| Chave | O que assina | Onde mora |
|---|---|---|
| Atualização (minisign) | o instalador do Windows (e do macOS, quando houver) | `~/auxilos/unkvoid.key`, só no WSL de quem publica |
| Repositório APT (GPG) | o índice de pacotes do Linux | `repo@unkvoid.com`, no chaveiro da VPS |

A pública da atualização vai **dentro do app**: `PUBLIC_KEY` em
`native/shared/core/src/update.rs` (a mesma de `plugins.updater.pubkey` no `tauri.conf.json`, que
o Tauri de antes usa). O par em uso tem o identificador `9a18c9243ef59b08`. Confira **antes** de
publicar, com `check-signature.mjs`: no WSL e no Windows, `~/.tauri/unkvoid.key` é um par
**antigo** (`3bc5c39b972ff1c3`), que assina sem reclamar e só é recusado na máquina de quem
instalou — foi assim que a 0.0.7 saiu.

A privada não vai para a VPS: quem a tiver publica atualização para todo mundo. Trocar a pública
quebra todo mundo que já instalou (precisa de uma instalação manual, uma última vez); se a
privada sumir, é o mesmo custo. Guarde uma cópia em lugar seguro.

## Onde o app procura

`https://unkvoid.com/downloads/latest.json`, montado pelo Laravel a cada pedido a partir da
tabela `releases`, com URLs do MinIO assinadas por 1 h (por isso sem cache). Cada plataforma
registra só a sua linha, pela API assinada do `publish-release.sh`; publicar uma não apaga a
outra.

A chave da plataforma inclui o formato do instalador: `windows-x86_64-nsis`,
`windows-x86_64-msi`, `darwin-aarch64`, `linux-x86_64-deb`. **A `version` do manifesto é uma só,
a mais nova entre as plataformas**, e só entram nele as plataformas nessa versão.

O app nativo lê o manifesto na abertura. Com ele aberto, quem avisa é o servidor: o
`POST /api/releases` manda `ReleasePublished { version, platform }` pelo canal público `releases`
do tempo real (ver [CONTRATO.md](CONTRATO.md)), que todo app aberto ouve, com conta ou sem.

## Windows

### Como o app se atualiza

`native/shared/core/src/update.rs`: compara pelo semver (`0.1.0` vem depois de `0.1.0-beta`),
baixa calado, confere a assinatura e mostra o botão verde na barra, ao lado do minimizar. Quem
escolhe a hora é a pessoa: ninguém cai da sala porque saiu uma versão. O clique guarda onde ela
está (`native/shared/core/src/resume.rs`: a sala por código ou o canal de voz, e a tela no ar),
se despede da sala e abre o instalador com `/P /UPDATE /R`; a versão nova volta para lá. O
guardado vale por 10 minutos e uma vez só.

Com os Clips o app roda com o nível mais alto da conta (ver [DECISOES.md](DECISOES.md)): numa
conta de administrador o instalador herda a elevação e troca **sem aviso do UAC**; numa conta
comum o Windows pede a senha do administrador. Aberto no logon, escondido na bandeja, o app passa
`/S /UPDATE /R /BACKGROUND`: nem a barra do instalador aparece por cima de um jogo.

O `installer.nsi` ocupa o lugar do Tauri (a mesma pasta, o mesmo `unkvoid-desktop.exe`, a mesma
chave de desinstalação): o atualizador do Tauri acha a versão nova, confere com a mesma chave e
migra a pessoa. O nativo não tem `.msi`: quem instalou o Tauri pelo `.msi` precisa do `.exe` uma
vez.

### Publicar

1. Suba `version` no `native/Cargo.toml` (workspace).
2. No Windows, o instalador ([BUILD-WINDOWS.md](BUILD-WINDOWS.md)):

   ```powershell
   powershell -ExecutionPolicy Bypass -File native\apps\windows\build-installer.ps1
   ```

3. No WSL, assinar, conferir e publicar — a chave não sai de lá. **Passe sempre
   `RELEASE_VERSION`**: sem ela o `publish-release.sh` lê a versão do `tauri.conf.json`.

   ```bash
   export PATH="$HOME/.nvm/versions/node/v24.19.0/bin:$PATH"   # o bash -lc do WSL não acha o node
   cd native/apps/desktop
   VERSION=0.1.16
   EXE=/mnt/c/Users/edsu/unkvoid/native/target/release/bundle/windows/Unkvoid_${VERSION}_x64-setup.exe
   npx tauri signer sign -f ~/auxilos/unkvoid.key -p "" "$EXE"
   node check-signature.mjs "$EXE.sig"
   RELEASE_VERSION=$VERSION RELEASE_SECRET="$(grep -h . ~/auxilos/release-secret.env | cut -d= -f2)" \
       ./publish-release.sh windows-x86_64-nsis "$EXE" "$EXE.sig"
   ```

4. Antes de publicar app nativo, entre numa sala contra a produção:
   `cargo run -p core-app --example room -- wss://unkvoid.com/sfu <código> watch 8`. Todo teste é
   na pilha local, que é `ws://`; a 0.1.0-beta saiu sem TLS e não abriu sala nenhuma.

Se a versão nova muda o que o SFU ou o site respondem, eles sobem **antes** do app.

## Linux

O `.deb` sai do `native/apps/linux/build-deb.sh` ([BUILD-LINUX.md](BUILD-LINUX.md)) e vai para o
APT pelo `apt-publish.sh`, na VPS, que refaz o índice e o assina com a GPG:

```bash
native/apps/linux/build-deb.sh
scp native/target/deb12/Unkvoid_0.1.16_amd64.deb vps:/tmp/
ssh vps 'cd /var/www/projects/unkvoid/native/apps/desktop && ./apt-publish.sh /tmp/Unkvoid_0.1.16_amd64.deb'
```

O repositório mora no bucket `apt` do MinIO, servido em `https://unkvoid.com/apt/`. Para deixar
só a versão nova: apague os `.deb` velhos com `mc rm local/apt/<arquivo>` e rode o
`apt-publish.sh` de novo, que refaz o índice com o que sobrou.

Para quem usa, uma vez só:

```bash
curl -fsSL https://unkvoid.com/apt/unkvoid.gpg | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] https://unkvoid.com/apt ./" | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

> O `build-linux.yml` e o `build-vps.sh` ainda geram o `.deb` do Tauri. Não rode.

## macOS

O app nativo (`native/apps/macos`) ainda não tem release: o `bundle.sh` gera o `Unkvoid.app`
([BUILD-MACOS.md](BUILD-MACOS.md)), mas nenhuma versão `darwin-aarch64` nativa foi publicada.
Hoje o app só **avisa** que há versão nova.

## Deixar só a versão nova no site

Cada publicação acrescenta uma linha em `releases`. Para apagar as antigas (instalador do MinIO
junto), mantendo a mais nova de cada plataforma, na VPS:

```bash
cd /var/www/projects/unkvoid-web/current && php artisan tinker --execute='
$keep = array_map(fn ($release) => $release->id, array_values(App\Models\Release::latestPerPlatform()));
foreach (App\Models\Release::whereNotIn("id", $keep)->get() as $release) { $release->remove(); }'
```

## Quando alguma coisa não atualiza

Na ordem, porque cada uma explica a seguinte:

1. `curl -s https://unkvoid.com/downloads/latest.json` — 404 quer dizer que nenhuma versão
   registrada tem assinatura (sem `.sig` ela fica de fora do manifesto).
2. A `version` do manifesto é maior que a instalada? Igual não atualiza.
3. A plataforma está lá? `windows-x86_64-nsis` para quem instalou pelo `.exe`.
4. A pública dentro do app bate com a privada que assinou? O `check-signature.mjs` responde.
5. O arquivo da `url` responde 200? A URL vence em uma hora: peça o manifesto de novo antes de
   concluir qualquer coisa.
6. No Linux, nenhuma das cinco: é `apt update && apt upgrade`. Na Microsoft Store, é a Store.
