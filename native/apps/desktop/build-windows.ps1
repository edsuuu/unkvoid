#Requires -Version 5
<#
  build-windows.ps1 - gera os instaladores assinados do Unkvoid no Windows.

  Sai um .msi e um .exe (NSIS). Os dois instalam perMachine, entao a atualizacao
  automatica passa pelo aviso de administrador do Windows antes de trocar os arquivos.

  Sincroniza o codigo do WSL, compila + assina e copia o resultado pra pasta de saida.

  Uso:
    powershell -ExecutionPolicy Bypass -File \\wsl.localhost\Ubuntu-26.04\var\www\projects\unkvoid\native\apps\desktop\build-windows.ps1
    ...\build-windows.ps1 -Out "D:\entrega"   (muda a pasta de saida)
    ...\build-windows.ps1 -Src "\\wsl.localhost\Ubuntu-26.04\..."  (compila outra copia)
#>
param(
  [string]$Src  = "\\wsl.localhost\Ubuntu-26.04\var\www\projects\unkvoid",
  [string]$Work = "$env:USERPROFILE\unkvoid-build",
  [string]$Out  = "$env:USERPROFILE\Desktop\apps",
  [string]$Key  = "$env:USERPROFILE\.tauri\unkvoid.key"
)
$ErrorActionPreference = "Stop"
function Assert-LastExit($msg) { if ($LASTEXITCODE -ne 0) { throw "$msg (codigo $LASTEXITCODE)" } }

# 1. CMake do Visual Studio Build Tools no PATH
$cmakeBin = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
if (Test-Path $cmakeBin) { $env:Path = "$cmakeBin;$env:Path" }
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
  throw "cmake nao encontrado. Instale: winget install Kitware.CMake  (e abra um PowerShell novo)"
}

# 2. Sincroniza o codigo do WSL (node_modules e target ficam, pra build incremental).
#    O .gitignore continua vindo; o que fica de fora e o diretorio de controle de versao,
#    que numa worktree e um arquivo apontando pra um caminho do Linux - do lado do
#    Windows ele nao leva a lugar nenhum.
if (-not (Test-Path $Src)) { throw "Codigo nao encontrado em $Src - o WSL esta ligado?" }
Write-Host "==> Sincronizando codigo de $Src" -ForegroundColor Cyan
robocopy $Src $Work /MIR /XD node_modules target .git dist .idea .vscode /XF .git /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy falhou (codigo $LASTEXITCODE)" }

Set-Location (Join-Path $Work "native\apps\desktop")

# 3. Dependencias (so quando faltam)
if (-not (Test-Path "node_modules")) {
  Write-Host "==> npm ci" -ForegroundColor Cyan
  npm ci; Assert-LastExit "npm ci falhou"
}

# 4. Assinatura. Sem a chave o build sai sem .sig, o instalador sobe igual e ninguem se
#    atualiza - a publicacao parece certa e nao atualiza ninguem. Por isso falha aqui.
if (-not (Test-Path $Key)) { throw "Sem $Key - copie a chave do auto-update do Mac antes de compilar" }
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw $Key
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""

# 5. Build
Write-Host "==> Compilando .msi e .exe (a primeira vez demora)" -ForegroundColor Cyan
npx tauri build --bundles nsis,msi; Assert-LastExit "tauri build falhou"

# 6. Copia o resultado
$bundle = "$Work\native\target\release\bundle"
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item "$bundle\msi\*.msi*" $Out -Force
Copy-Item "$bundle\nsis\*-setup.exe*" $Out -Force

# 7. A conferencia que nao da pra pular: um instalador sem .sig ao lado sobe sem erro
#    nenhum e some do manifesto depois, longe daqui.
$installers = Get-ChildItem "$Out\*.msi", "$Out\*-setup.exe"
foreach ($file in $installers) {
  if (-not (Test-Path "$($file.FullName).sig")) { throw "$($file.Name) saiu sem .sig - ninguem se atualiza para ele" }
}

Write-Host "`n==> Pronto. Arquivos em $Out :" -ForegroundColor Green
$installers | Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,1)}}
Write-Host "`nPara publicar, no WSL: make publish-windows" -ForegroundColor Cyan
