# Atualização automática e publicação

Como sair uma versão para os três sistemas, e o que faz o app se atualizar sozinho.

O app procura versão nova ao abrir e de seis em seis horas. Se achar, baixa,
instala e reinicia sozinho — a menos que você esteja numa sala, porque reiniciar
no meio de uma transmissão derrubaria quem está assistindo. Nesse caso a versão
fica no disco e passa a valer no próximo reinício.

**No Linux isso não acontece, de propósito.** Lá quem atualiza é o APT, junto com
o resto da máquina. Ver a seção do Linux.

## As duas chaves, que são diferentes

Confundir uma com a outra é o erro mais caro deste arquivo.

| Chave | O que assina | Onde mora |
|---|---|---|
| Auto-update (minisign) | O instalador do macOS e do Windows | `~/auxilos/unkvoid.key`, só nas máquinas que buildam |
| Repositório APT (GPG) | O índice de pacotes do Linux | `repo@unkvoid.com`, no chaveiro da VPS |

A pública do auto-update vai **dentro do app**, em
`native/apps/desktop/src-tauri/tauri.conf.json`, em `plugins.updater.pubkey`. A
privada assina cada instalador e produz o `.sig` ao lado dele.

O par em uso tem o identificador `9a18c9243ef59b08`. Confira antes de assinar, e
não pelo caminho do arquivo: no WSL e na máquina do Windows, `~/.tauri/unkvoid.key`
é um par **antigo**, de identificador `3bc5c39b972ff1c3`. Ele assina sem reclamar,
o build sai com um aviso no meio de mil linhas, a publicação parece certa, e a
recusa só acontece na máquina de quem instalou. Foi assim que a 0.0.7 do Windows
foi publicada: assinada com uma chave que o app não reconhece. O
`build-windows.ps1` compara os identificadores e falha quando não batem.

**A privada do auto-update não fica na VPS.** Quem a tiver publica atualização
para todo mundo que instalou o app, e a VPS é uma máquina exposta à internet. O
Linux não precisa dela: quem autentica o pacote lá é a GPG do repositório.

Trocar a pública quebra todo mundo que já instalou: o app compara a assinatura
com a chave que ele carrega por dentro, então quem tem a versão antiga precisa de
uma instalação manual, uma última vez. A anterior está guardada em
`~/.tauri/unkvoid-B1BAB-antiga.key.bak`.

Se a privada sumir, a única saída é gerar outro par e trocar a pública — com o
mesmo custo. Faça uma cópia em lugar seguro.

## Onde o app procura

`https://unkvoid.com/downloads/latest.json`, montado pelo Laravel a cada pedido a
partir da tabela `releases`, com `Cache-Control: no-store`. O instalador em si
mora no MinIO, e a URL do manifesto é assinada na hora e vence em uma hora — por
isso o manifesto não pode ser guardado em cache.

O manifesto nasce do banco, então nenhuma publicação apaga a outra: cada sistema
é compilado numa máquina diferente e registra só a sua linha, pela API assinada
de `publish-release.sh`. Publicar o Windows não tira o macOS que subiu ontem.

A chave de cada plataforma é a que o atualizador do Tauri procura, e o instalador
faz parte dela: `windows-x86_64-msi`, `windows-x86_64-nsis`, `darwin-aarch64`,
`linux-x86_64-deb`. Não é enfeite — o app confere os bytes do que baixou, e um
`.exe` entregue a quem instalou pelo `.msi` morre em `InvalidUpdaterFormat`.

**A versão do manifesto é uma só, a mais nova entre todas as plataformas.** Subir
o Windows para uma versão que o macOS ainda não tem faz todo Mac instalado baixar
o `.app.tar.gz` velho, se atualizar para a versão que já tinha, e repetir. Os três
sistemas saem na mesma versão, ou o que sair sozinho espera.

## macOS

Na máquina com Xcode e a chave privada:

```bash
cd native/apps/desktop
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/unkvoid.key)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
npm run tauri build

RELEASE_SECRET="$(grep -h . ~/auxilos/release-secret.env | cut -d= -f2)" \
./publish-release.sh darwin-aarch64 \
  ../../target/release/bundle/macos/Unkvoid.app.tar.gz \
  ../../target/release/bundle/macos/Unkvoid.app.tar.gz.sig
```

O artefato do atualizador é o `.app.tar.gz`, **não** o `.dmg`. O `.dmg` é para
quem instala pela primeira vez.

Se o empacotamento falhar com `bundle_dmg.sh`, sobrou um volume montado de um
build interrompido:

```bash
hdiutil detach /Volumes/dmg.* -force
rm -f native/target/release/bundle/macos/rw.*.dmg
```

## Windows

### O app nativo, da 0.1.0-beta em diante

O Windows passou a ser o app nativo (`native/apps/windows`, Slint), e não o Tauri. O
instalador é o `native/apps/windows/installer.nsi`, um NSIS que ocupa o lugar exato do Tauri:
a mesma pasta em Arquivos de Programas, o mesmo `unkvoid-desktop.exe`, a mesma chave de
desinstalação. É isso que migra quem ainda tem o Tauri — o atualizador dele acha a versão
nova no manifesto, confere a assinatura com a mesma chave e abre o instalador com
`/P /UPDATE /R /ARGS …`, que o `installer.nsi` entende. O mesmo nome de `.exe` mantém o
atalho do menu Iniciar e o app fixado na barra de tarefas valendo.

O nativo se atualiza do mesmo jeito, pelo `native/shared/core/src/update.rs`: na abertura lê
o manifesto, compara pelo semver (`0.1.0` vem depois de `0.1.0-beta`), baixa com a barra na
tela, confere a assinatura com a mesma pública do `tauri.conf.json` e abre o instalador com
`/P /UPDATE /R`. Recusar o aviso do administrador só deixa a versão que está.

Compilar e empacotar, no PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File native\apps\windows\build-installer.ps1
```

Assinar e publicar, no WSL — a chave não sai de lá:

```bash
cd native/apps/desktop
EXE=/mnt/c/Users/edsu/unkvoid/native/target/release/bundle/windows/Unkvoid_0.1.0-beta_x64-setup.exe
npx tauri signer sign -f ~/auxilos/unkvoid.key -p "" "$EXE"
node check-signature.mjs "$EXE.sig"
RELEASE_VERSION=0.1.0-beta RELEASE_SECRET="$(grep -h . ~/auxilos/release-secret.env | cut -d= -f2)" ./publish-release.sh windows-x86_64-nsis "$EXE" "$EXE.sig"
```

O nativo não tem `.msi`: quem instalou o Tauri pelo `.msi` não migra sozinho, e precisa do
`.exe` uma vez. O `release.yml` e o `build-windows.ps1` ainda compilam o Tauri — não solte
tag por eles, ou o Tauri volta por cima do nativo.

### De onde saía a release do Tauri

**Do `release.yml`, num runner do GitHub, a partir do próprio repositório** — e não da máquina
de ninguém. É o que a assinatura de código exige (a SignPath Foundation só assina build
automatizado e verificável; a política está no [README](../README.md#code-signing-policy)), e
funciona de novo desde que o repositório ficou público: runner padrão é de graça ali.

```bash
# a versão do tauri.conf.json já subiu e está na main
git tag v0.0.39 && git push origin v0.0.39          # compila, confere a assinatura e publica o Windows

gh workflow run release.yml -f platform=windows -f publish=false   # só compila: os instaladores ficam como artefato do run
gh workflow run release.yml -f platform=macos                      # o macOS, só por disparo à mão
```

A tag tem de bater com a versão do `tauri.conf.json`, senão o fluxo para antes de compilar. Os
secrets do repositório que ele usa: `TAURI_SIGNING_PRIVATE_KEY` (o par `9a18c9243ef59b08`; para
trocar, `gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/auxilos/unkvoid.key`) e `RELEASE_SECRET`. O
passo `check-signature.mjs` derruba o fluxo **antes de publicar** se a chave do secret não for a
do app.

Quando a SignPath aprovar o projeto, a assinatura Authenticode entra entre o build e a
publicação, nesta ordem: compilar → guardar os instaladores como artefato do run (a SignPath só
aceita o que veio dali) → pedir a assinatura e esperar a aprovação manual → **refazer o `.sig` do
atualizador** em cima do instalador já assinado (`npx tauri signer sign`), porque assinar muda os
bytes e o `.sig` antigo deixa de valer → publicar. O artefato do run já existe; o resto depende
dos identificadores que a SignPath entrega na aprovação.

O build local (`build-windows.ps1`, abaixo) continua existindo para testar um instalador antes
de soltar a tag, e como saída de emergência.

### Os dois instaladores

Saem dois instaladores, e os dois são publicados: o `.exe` do NSIS, que é o que a
pessoa baixa do site, e o `.msi`, que é o que a empresa instala por política. Cada
um vira uma chave própria no manifesto, porque o app se atualiza pelo mesmo
formato por onde foi instalado.

O `.msi` exige o WiX e o `.exe` exige o NSIS; os dois só rodam no Windows. O
código é editado no WSL e sincronizado para `C:\Users\edsu\unkvoid-build` pelo
script de build.

### O que a pessoa vê

Os dois instalam para a máquina toda, em `Arquivos de Programas`. É de propósito:
trocar arquivo lá exige administrador, então o Windows abre o aviso do UAC e a
atualização só segue quando a pessoa confirma. Instalar por usuário tiraria o
aviso e trocaria o app sem perguntar.

O app procura versão nova assim que abre e depois de seis em seis horas. Achando,
baixa com a barra de progresso na tela, dispara o instalador e sai — o instalador
põe a versão nova no lugar e reabre o app. Dentro de uma sala nada disso acontece:
a atualização espera a próxima abertura, porque no Windows o processo morre junto
com a chamada e a transmissão cairia com ele.

### A chave não se copia para cá

Nada a preparar: o script lê `~/auxilos/unkvoid.key` do WSL, pelo mesmo caminho de
rede por onde lê o código. **Não copie a chave para `%USERPROFILE%\.tauri`** — foi
de lá que saiu a 0.0.7 assinada com o par errado, e o arquivo que ainda está lá é
o par antigo.

Sem a chave, ou com a chave errada, o build para com erro. É de propósito: um
build sem `.sig`, ou com `.sig` de outro par, sobe igual e não atualiza ninguém,
enquanto a publicação parece certa.

### O build local

No PowerShell, para compilar e assinar — o script lê o código direto do WSL, então
não há nada a copiar antes:

```powershell
powershell -ExecutionPolicy Bypass -File \\wsl.localhost\Ubuntu-26.04\var\www\projects\unkvoid\native\apps\desktop\build-windows.ps1
```

Ele põe o CMake no PATH, sincroniza o código, roda `npm ci` se faltar, assina com
a chave, empacota o `.msi` e o `.exe`, confere que cada um saiu com o `.sig` ao
lado e copia tudo para `C:\Users\edsu\Desktop\apps`.

De volta no WSL, para publicar. O script é bash, então não roda no PowerShell:

```bash
make publish-windows
```

Ele lê a versão do `tauri.conf.json`, pega os dois instaladores dessa versão e
registra cada um com a sua chave. A pasta de saída guarda build de todas as
versões, e o filtro existe para não subir o `.msi` de ontem com o número de hoje.

## Linux

Da 0.1.0-beta em diante o `.deb` é o do app nativo (`native/apps/linux`, GTK4), com o mesmo
nome de pacote, `unkvoid`: quem tem o Tauri recebe o nativo no próximo `apt upgrade`. Ele
sai do `native/apps/linux/build-deb.sh`, compilado dentro de um Debian 12
(`Dockerfile.deb`), e vai para o APT pelo mesmo `apt-publish.sh`, na VPS:

```bash
native/apps/linux/build-deb.sh     # native/target/deb12/Unkvoid_<versão>_amd64.deb
scp native/target/deb12/Unkvoid_0.1.0-beta_amd64.deb vps:/tmp/
ssh vps 'cd /var/www/projects/unkvoid/native/apps/desktop && ./apt-publish.sh /tmp/Unkvoid_0.1.0-beta_amd64.deb'
```

A versão do Cargo `0.1.0-beta` vira `0.1.0~beta` no Debian: com o til, a `0.1.0` final vem
depois da beta. O `build-vps.sh` e o `build-linux.yml` ainda geram o `.deb` do Tauri.

**Instala e atualiza pelo APT**, e nada mais. O atualizador embutido está
desligado neste sistema: pedir senha de root com `pkexec` no meio da abertura
faria o que o `apt upgrade` já faz junto com o resto da máquina.

Para quem usa, uma vez só:

```bash
curl -fsSL https://unkvoid.com/apt/unkvoid.gpg \
  | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] https://unkvoid.com/apt ./" \
  | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

Depois disso, versão nova chega com `sudo apt upgrade`, como qualquer outro
pacote.

Publicar é automático: todo push na `main` que mexe em `native/` dispara o
`build-linux.yml` no runner da própria VPS, que compila dentro de um Debian 12
(`Dockerfile.linux`), gera o `.deb` e chama o `apt-publish.sh`, que refaz o índice e
o assina com a GPG do repositório. A chave do auto-update não viaja: o Linux não a
usa. A versão que sai é a do `tauri.conf.json`, então subir o número faz parte do
mesmo PR.

À mão, de qualquer máquina com acesso por SSH à VPS, o mesmo build:

```bash
make build-vps
```

O build compartilha a máquina com o SFU, então roda com um núcleo de folga e em
prioridade baixa — alguns minutos a mais, e nenhuma transmissão engasgando no
meio.

### O AppImage

Sai só com `UNKVOID_BUNDLES=deb,appimage`, e **ninguém o atualiza sozinho**: ele
não tem gerenciador de pacotes e o app não consulta o manifesto no Linux. Serve
para distro sem APT, e quem o usa troca o arquivo à mão.

## GitHub

Fora do caminho de atualização. O `build-vps.sh` só publica release lá com
`--github`, e é arquivo para quem quiser baixar à mão. Sem a flag ele termina
depois do APT, que é o que importa.

## Quando alguma coisa não atualiza

Na ordem, porque cada uma explica a seguinte:

1. `curl -s https://unkvoid.com/downloads/latest.json` — se der 404, nada mais
   importa: o app pergunta e não recebe resposta. Dá 404 quando nenhuma versão
   registrada tem assinatura, porque uma sem `.sig` fica de fora do manifesto.
2. A `version` do manifesto é maior que a instalada? Igual não atualiza.
3. A plataforma está lá? `windows-x86_64-nsis` para quem instalou pelo `.exe`,
   `windows-x86_64-msi` para quem instalou pelo `.msi`, `darwin-aarch64` no Mac.
   O app procura primeiro a chave com o instalador e só depois `windows-x86_64`.
4. A `pubkey` do `tauri.conf.json` bate com a privada que assinou? Se o build
   avisou `does not match the public key`, a assinatura vai ser recusada em
   execução e o build passa mesmo assim.
5. O arquivo da `url` responde 200? A URL é assinada e vence em uma hora — se a
   que você tem na mão é de ontem, peça o manifesto de novo antes de concluir
   qualquer coisa.
6. No Linux, nenhuma das cinco: lá é `apt update && apt upgrade`.
