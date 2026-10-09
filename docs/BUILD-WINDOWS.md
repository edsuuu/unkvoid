# Build e instalação no Windows

O app do Windows é o nativo (`native/apps/windows`, Slint). Tudo aqui roda **no Windows**: o
Opus e o NSIS não se geram de dentro do WSL.

## Preparar a máquina

- Visual Studio Build Tools 2022 com "Desenvolvimento para desktop com C++" (MSVC e Windows
  SDK). O `cmake` dele fica fora do PATH; o `build-installer.ps1` o acha sozinho, e para o
  `cargo` à mão é `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`.
- Rust (rustup, alvo `x86_64-pc-windows-msvc`).
- O NSIS em `%LOCALAPPDATA%\tauri\NSIS\makensis.exe` (o que o Tauri baixa na primeira build
  dele).

## Compilar e testar

```powershell
cd native
cargo clippy --workspace --exclude unkvoid-linux --all-targets -- -D warnings
cargo test --workspace --exclude unkvoid-linux
```

O `unkvoid-linux` (GTK4) não compila no Windows; por isso o `--exclude`. Para não roubar CPU de
quem está usando a máquina: `cmd /c "start /low /b /wait cargo ..."`.

## O instalador do site

```powershell
powershell -ExecutionPolicy Bypass -File native\apps\windows\build-installer.ps1
```

Compila em release e empacota o `native/apps/windows/installer.nsi`:
`native/target/release/bundle/windows/Unkvoid_<versão>_x64-setup.exe`. A versão é a do
`native/Cargo.toml`. Sai **sem** o `.sig`: assinar e publicar é no WSL, onde mora a chave
([AUTO-UPDATE.md](AUTO-UPDATE.md#windows)).

O instalador põe o app em `Arquivos de Programas`, no mesmo lugar e com o mesmo
`unkvoid-desktop.exe` do Tauri de antes — é assim que quem tinha o Tauri migra. Por instalar para
a máquina toda, ele pede o aviso de administrador.

> O `release.yml` e o `native/apps/desktop/build-windows.ps1` ainda compilam o **Tauri**. Não
> solte tag nem rode nenhum dos dois para publicar: o Tauri voltaria por cima do nativo.

## O pacote da Microsoft Store (MSIX)

```powershell
powershell -ExecutionPolicy Bypass -File native\apps\windows\build-msix.ps1
```

Sai `native/target/release/bundle/windows/Unkvoid_<versão>.0_x64.msix`, sem assinatura: é esse
arquivo que sobe no Partner Center (produto `9NGPGTV3NPLW`), e a Store assina depois de aprovar.
A identidade do pacote (`Unkvoid.Unkvoid`, `CN=3877BA03-…`) está em
`native/apps/windows/msix/AppxManifest.xml`; o quarto número da versão é da Store, sempre 0, e
cada envio precisa de versão maior que a anterior.

O mesmo `unkvoid.exe` muda de jeito quando roda como pacote (`shell::packaged()`): não procura
versão no site (quem atualiza é a Store), abre no logon pela `StartupTask` do manifesto e **não
se eleva** — a Store não aprova pacote que pede administrador. Nessa instalação os atalhos dos
Clips e o falar-apertando não alcançam jogos que rodam elevados (anti-cheat).

## Quando algo dá errado

- **Prévia ou transmissão preta:** confira a permissão de captura e o driver de vídeo. No
  Windows 10 o monitor vai pelo Desktop Duplication (sem a borda amarela, ver
  [BORDA-AMARELA.md](BORDA-AMARELA.md)); `UNKVOID_DUPLICATION=on` força esse caminho no 11.
- **Sem encoder na placa:** o app cai sozinho para o de software em 720p30;
  `UNKVOID_ENCODER=cpu` força esse caminho para comparar.
- **O que aconteceu:** o log do dia em `%LOCALAPPDATA%\com.unkvoid.desktop\unkvoid-AAAA-MM-DD.log`.
  As linhas com `ERROR` também chegam ao site, na tabela `error_reports`.
