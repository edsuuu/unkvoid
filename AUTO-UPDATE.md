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
| Auto-update (minisign) | O instalador do macOS e do Windows | `~/.tauri/unkvoid.key`, só nas máquinas que buildam |
| Repositório APT (GPG) | O índice de pacotes do Linux | `repo@unkvoid.com`, no chaveiro da VPS |

A pública do auto-update vai **dentro do app**, em
`native/apps/desktop/src-tauri/tauri.conf.json`, em `plugins.updater.pubkey`. A
privada assina cada instalador e produz o `.sig` ao lado dele.

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

`https://discord.unkvoid.com/downloads/latest.json`, servido pelo nginx a partir
de `/var/www/downloads/unkvoid/` com `Cache-Control: no-store`.

O manifesto é **costurado**, não sobrescrito. Cada sistema é compilado numa
máquina diferente, e as três chamam o mesmo `publish-downloads.sh`: publicar o
Windows não pode apagar o macOS que subiu ontem.

## macOS

Na máquina com Xcode e a chave privada:

```bash
cd native/apps/desktop
TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/unkvoid.key)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
npm run tauri build

./publish-downloads.sh 0.0.7 darwin-aarch64 \
  ../../target/release/bundle/macos/Unkvoid.app.tar.gz
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

O `.msi` exige o WiX, que só roda no Windows. O código é editado no WSL e
sincronizado para `C:\Users\edsu\unkvoid-build` pelo script de build.

### Uma vez só

Copie a chave privada do Mac para a máquina Windows, em
`%USERPROFILE%\.tauri\unkvoid.key`. Sem ela o build sai sem `.sig`, o instalador
sobe igual e ninguém se atualiza — a publicação parece certa e não atualiza
ninguém.

### A cada versão

No WSL, para o código chegar:

```bash
cd ~/projects/unkvoid && git pull
```

No PowerShell, para compilar e assinar:

```powershell
powershell -ExecutionPolicy Bypass -File C:\Users\edsu\build-unkvoid.ps1
```

Ele põe o CMake no PATH, sincroniza o código do WSL, roda `npm ci` se faltar,
assina com a chave se ela existir, empacota o `.msi` e copia o resultado para
`C:\Users\edsu\Desktop\apps`.

De volta no WSL, para publicar. O script é bash, então não roda no PowerShell:

```bash
cd ~/projects/unkvoid/native/apps/desktop
./publish-downloads.sh 0.0.7 windows-x86_64 \
  /mnt/c/Users/edsu/Desktop/apps/Unkvoid_0.0.7_x64_pt-BR.msi
```

O `.msi.sig` precisa estar na mesma pasta que o `.msi`. O script recusa sem ele,
de propósito.

Troque `0.0.7` pela versão do `tauri.conf.json` e o nome do arquivo pelo que o
build gerou — o sufixo muda com o idioma do instalador.

## Linux

**Instala e atualiza pelo APT**, e nada mais. O atualizador embutido está
desligado neste sistema: pedir senha de root com `pkexec` no meio da abertura
faria o que o `apt upgrade` já faz junto com o resto da máquina.

Para quem usa, uma vez só:

```bash
curl -fsSL https://discord.unkvoid.com/apt/unkvoid.gpg \
  | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] https://discord.unkvoid.com/apt ./" \
  | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

Depois disso, versão nova chega com `sudo apt upgrade`, como qualquer outro
pacote.

Para publicar uma versão, do Mac:

```bash
make build-vps
```

Ele atualiza o clone na VPS, compila lá, gera o `.deb` e chama o
`apt-publish.sh`, que refaz o índice e o assina com a GPG do repositório. A
chave do auto-update não viaja: o Linux não a usa.

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

1. `curl -s https://discord.unkvoid.com/downloads/latest.json` — se der 404,
   nada mais importa: o app pergunta e não recebe resposta.
2. A `version` do manifesto é maior que a instalada? Igual não atualiza.
3. A plataforma está lá? `darwin-aarch64`, `windows-x86_64`.
4. A `pubkey` do `tauri.conf.json` bate com a privada que assinou? Se o build
   avisou `does not match the public key`, a assinatura vai ser recusada em
   execução e o build passa mesmo assim.
5. O arquivo da `url` responde 200?
6. No Linux, nenhuma das cinco: lá é `apt update && apt upgrade`.
