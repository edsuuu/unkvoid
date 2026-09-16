# Instaladores

Gerados na máquina de quem tem o sistema — **não há CI**, e instalador não cross-compila:
`.msi` e `.exe` só saem no Windows, `.dmg` só no macOS.

| Arquivo | O que é |
|---|---|
| `Unkvoid_<versão>_x64-setup.exe` | instalador do Windows (NSIS), o mais comum |
| `Unkvoid_<versão>_x64_pt-BR.msi` | instalador do Windows (MSI), para quem instala por política |
| `Unkvoid_<versão>_x64_portatil.exe` | o app solto: abre sem instalar, bom para testar rápido |

## Estes daqui não têm assinatura

Foram gerados com `createUpdaterArtifacts: false` porque a chave privada
(`~/.tauri/unkvoid.key`) está **no Mac**. Eles instalam e rodam normalmente; o que não
fazem é servir de alvo para a atualização automática de quem já tem o app.

Para gerar uma versão que atualize sozinha, a chave precisa estar na máquina do build:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $HOME\.tauri\unkvoid.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
npx tauri build
```

A chave pública já está no `tauri.conf.json` e as duas precisam bater — trocar uma sem a
outra deixa todo mundo instalado sem para onde atualizar.

## Como regerar

Windows, do próprio Windows (PowerShell), na cópia do repositório que vive no disco C:

```powershell
cd C:\Users\edsu\unkvoid-build\native\apps\desktop
npm ci
npx tauri build --bundles nsis,msi
```

macOS:

```bash
cd native/apps/desktop && npx tauri build --bundles app dmg
```

O contexto de por que existe uma cópia em `C:\Users\edsu\unkvoid-build` está no
[BUILD-WINDOWS.md](../docs/BUILD-WINDOWS.md).
