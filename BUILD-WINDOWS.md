# Build e instalação no Windows

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
