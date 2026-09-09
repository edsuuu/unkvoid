# Build e instalação no Windows

A cópia usada para gerar o instalador fica em `C:\Users\edsu\unkvoid-build`.
O build deve ser executado no Windows para que o Tauri use os binários nativos
corretos.

## Gerar e abrir o instalador

No PowerShell:

```powershell
cd C:\Users\edsu\unkvoid-build\native\apps\desktop
npm ci
npx tauri build --bundles nsis,msi
```

Para abrir o instalador NSIS recém-gerado:

```powershell
$installer = Get-ChildItem ..\..\..\target\release\bundle\nsis\*-setup.exe |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
Start-Process $installer.FullName
```

Para abrir o MSI:

```powershell
$msi = Get-ChildItem ..\..\..\target\release\bundle\msi\*.msi |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
Start-Process msiexec.exe -ArgumentList "/i `"$($msi.FullName)`""
```

## Compartilhamento de tela

O cliente fecha os producers de vídeo e áudio no SFU antes de parar a captura.
Isso libera a porta UDP imediatamente e atualiza a tela dos espectadores sem
aguardar a desconexão da sala. Falhas durante o início também encerram a captura
parcial, permitindo uma nova tentativa sem o erro `a stream is already in progress`.

Se o preview do Windows aparecer preto, confirme que o aplicativo tem permissão
para captura de tela e que o driver gráfico está atualizado. O preview usa
Windows Graphics Capture e precisa de um monitor ou janela válido.

