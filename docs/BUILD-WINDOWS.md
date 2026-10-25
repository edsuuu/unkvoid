# Build e instalação no Windows

O app único está provisoriamente em `native/apps/unkvoid`; o diretório será renomeado para
`native/apps/unkvoid`. Tudo aqui roda no Windows: o Opus e o instalador NSIS não saem do WSL.

## Preparar a máquina

- Visual Studio Build Tools 2022 com "Desenvolvimento para desktop com C++" (MSVC e Windows
  SDK). O `build-installer.ps1` encontra o CMake do Build Tools.
- Rust com o alvo `x86_64-pc-windows-msvc`.
- NSIS 3 instalado em `C:\Program Files (x86)\NSIS\makensis.exe` (por exemplo,
  `choco install nsis`). Ele é instalado separadamente para o bundle não depender do Tauri.

## Compilar e testar

```powershell
cd native
cargo clippy --workspace --exclude unkvoid-linux --all-targets -- -D warnings
cargo test --workspace --exclude unkvoid-linux
```

## O instalador do site

```powershell
powershell -ExecutionPolicy Bypass -File native\apps\unkvoid\build-installer.ps1
```

O script compila em release e cria
`native/target/release/bundle/windows/Unkvoid_<versão>_x64-setup.exe`. A versão vem de
`[workspace.package]` em `native/Cargo.toml`; os ícones estão em `native/apps/unkvoid/icons/`.
O `.sig` é criado depois, no ambiente que guarda a chave privada: veja
[AUTO-UPDATE.md](AUTO-UPDATE.md#windows).

## O pacote da Microsoft Store (MSIX)

```powershell
powershell -ExecutionPolicy Bypass -File native\apps\unkvoid\build-msix.ps1
```

O resultado é `native/target/release/bundle/windows/Unkvoid_<versão>.0_x64.msix`, sem assinatura:
ele sobe no Partner Center e a Store o assina depois de aprovar. A Store atualiza esse pacote;
o site publica apenas o `.exe`.

## Quando algo dá errado

- **Prévia ou transmissão preta:** confira a permissão de captura e o driver de vídeo.
- **Sem encoder na placa:** `UNKVOID_ENCODER=cpu` força o caminho de software para comparar.
- **O que aconteceu:** o log do dia fica em `%LOCALAPPDATA%\com.unkvoid.desktop\` e as linhas
  com `ERROR` chegam à tabela `error_reports` do site.
