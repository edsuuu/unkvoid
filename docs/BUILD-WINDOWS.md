# Build e instalação no Windows

> **A release de verdade sai do `release.yml`**, num runner do GitHub (ver
> [AUTO-UPDATE.md](AUTO-UPDATE.md#de-onde-sai-a-release)). O que está aqui é o build **local**:
> para testar um instalador antes de soltar a tag, e como saída de emergência.

Saem dois instaladores da mesma compilação: o `.exe` do NSIS, que é o que a pessoa
baixa do site, e o `.msi` do WiX, que é o que se instala por política de rede. Os
dois precisam rodar no Windows — nenhum dos dois se gera em Linux.

O código é editado no WSL. O script de build sincroniza uma cópia para
`C:\Users\edsu\unkvoid-build` e compila lá, para o Tauri usar os binários nativos
e o cache do cargo do próprio Windows.

## O caminho normal

```powershell
powershell -ExecutionPolicy Bypass -File \\wsl.localhost\Ubuntu-26.04\var\www\projects\unkvoid\native\apps\desktop\build-windows.ps1
```

Ele põe o CMake do Visual Studio Build Tools no PATH, sincroniza o código, roda
`npm ci` se faltar, assina com `%USERPROFILE%\.tauri\unkvoid.key`, empacota o
`.msi` e o `.exe`, confere que cada um saiu com o `.sig` ao lado e copia tudo
para `C:\Users\edsu\Desktop\apps`.

Sem a chave ele para antes de compilar. É de propósito: um build sem `.sig` gera
instaladores que funcionam e não atualizam ninguém, e a publicação depois parece
certa.

Para publicar no site, de volta no WSL: `make publish-windows`. O resto está em
[AUTO-UPDATE.md](AUTO-UPDATE.md).

## À mão, quando só se quer olhar o build

```powershell
cd C:\Users\edsu\unkvoid-build\native\apps\desktop
npm ci
npx tauri build --bundles nsis,msi
```

Sai sem assinatura, então serve para testar a instalação e não para publicar.

Para abrir o instalador NSIS recém-gerado:

```powershell
$installer = Get-ChildItem ..\..\target\release\bundle\nsis\*-setup.exe |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
Start-Process $installer.FullName
```

Para abrir o MSI:

```powershell
$msi = Get-ChildItem ..\..\target\release\bundle\msi\*.msi |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
Start-Process msiexec.exe -ArgumentList "/i `"$($msi.FullName)`""
```

Os dois instalam para a máquina toda, em `Arquivos de Programas`, e por isso
abrem o aviso de administrador do Windows. É o mesmo aviso que aparece quando o
app se atualiza sozinho — instalar por usuário tiraria a pergunta e trocaria o
app sem avisar.

## Compartilhamento de tela

O cliente fecha os producers de vídeo e áudio no SFU antes de parar a captura.
Isso libera a porta UDP imediatamente e atualiza a tela dos espectadores sem
aguardar a desconexão da sala. Falhas durante o início também encerram a captura
parcial, permitindo uma nova tentativa sem o erro `a stream is already in progress`.

Se o preview do Windows aparecer preto, confirme que o aplicativo tem permissão
para captura de tela e que o driver gráfico está atualizado. O preview usa
Windows Graphics Capture e precisa de um monitor ou janela válido.

## Nesta máquina: o código no WSL, o Rust no Windows

O repositório de verdade mora no WSL (`/var/www/projects/unkvoid`). O Rust e o
instalador do Windows rodam do lado de lá, numa cópia só do `native/` em
`C:\Users\edsu\unkvoid-build`: o `node_modules` do WSL traz o `@tauri-apps/cli` de
Linux, e rodar `npx tauri build` de lá pelo Windows não funciona. A cópia é
descartável; para atualizar:

```bash
rsync -a --delete --exclude node_modules --exclude target --exclude dist \
    /var/www/projects/unkvoid/native/ /mnt/c/Users/edsu/unkvoid-build/native/
```

Montado na máquina: Visual Studio Build Tools 2022 (carga C++, MSVC 14.44, Windows
SDK 10.0.26100), Rust e Node dos dois lados, e o alvo `x86_64-pc-windows-msvc` no WSL.

Para compilar o Rust do Windows sem gerar instalador, o `C:\Users\edsu\cargo-win.cmd`
aceita os mesmos argumentos do cargo:

```powershell
C:\Users\edsu\cargo-win.cmd clippy --workspace --all-targets -- -D warnings
C:\Users\edsu\cargo-win.cmd test --workspace
```

Do WSL é o mesmo script chamado por fora, e o `cd /mnt/c` é do bash do WSL (no
PowerShell ele vira `C:\mnt\c` e falha):

```bash
cd /mnt/c && cmd.exe /c "C:\Users\edsu\cargo-win.cmd check --workspace --all-targets"
```

O script faz três coisas que não são opcionais: mapeia o repositório do WSL para `Y:`
(o `cmd.exe` não aceita caminho UNC como diretório atual), põe no PATH o CMake que veio
no Build Tools (o `opusic-sys` precisa dele) e aponta `CARGO_TARGET_DIR` para
`C:\Users\edsu\unkvoid-target` (compilar pelo `Y:` falha no lock do compilador
incremental).

Do lado do WSL dá para conferir só o `capture`, que é Rust puro:
`cd native && cargo check --target x86_64-pc-windows-msvc -p capture`. O `media` não
dá: o `opusic-sys` compila C e precisa do MSVC.
