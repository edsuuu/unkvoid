#Requires -Version 5
<#
  build-unkvoid.ps1 - gera o instalador MSI assinado do Unkvoid no Windows.

  Sincroniza o codigo do WSL, compila + assina o MSI e copia pra pasta de saida.

  Uso:
    powershell -ExecutionPolicy Bypass -File C:\Users\edsu\build-unkvoid.ps1
    ...\build-unkvoid.ps1 -Out "D:\entrega"   (muda a pasta de saida)
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

# 2. Sincroniza o codigo do WSL (node_modules e target ficam, pra build incremental)
if (-not (Test-Path $Src)) { throw "Codigo nao encontrado em $Src - o WSL esta ligado?" }
Write-Host "==> Sincronizando codigo de $Src" -ForegroundColor Cyan
robocopy $Src $Work /MIR /XD node_modules target .git dist .idea .vscode /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy falhou (codigo $LASTEXITCODE)" }

Set-Location (Join-Path $Work "native\apps\desktop")

# 3. Dependencias (so quando faltam)
if (-not (Test-Path "node_modules")) {
  Write-Host "==> npm ci" -ForegroundColor Cyan
  npm ci; Assert-LastExit "npm ci falhou"
}

# 4. Assinatura
if (Test-Path $Key) {
  $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw $Key
  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
  Write-Host "==> Chave encontrada: MSI sera assinado (auto-update ok)" -ForegroundColor Green
} else {
  Write-Host "==> AVISO: sem $Key - MSI SEM assinatura (sem auto-update)" -ForegroundColor Yellow
}

# 5. Build
Write-Host "==> Compilando MSI (a primeira vez demora)" -ForegroundColor Cyan
npx tauri build --bundles msi; Assert-LastExit "tauri build falhou"

# 6. Copia o resultado
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item "$Work\native\target\release\bundle\msi\*.msi*" $Out -Force
Write-Host "`n==> Pronto. Arquivos em $Out :" -ForegroundColor Green
Get-ChildItem "$Out\*.msi*" | Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,1)}}
explorer $Out
