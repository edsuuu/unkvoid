# Compila o app nativo do Windows e empacota o instalador (installer.nsi).
#
# Assinar e publicar é no WSL, onde mora a chave do atualizador — ela não se copia para cá.
# O passo a passo está em docs/AUTO-UPDATE.md.
#
#   powershell -ExecutionPolicy Bypass -File native\apps\windows\build-installer.ps1
$ErrorActionPreference = 'Stop'

$native = Resolve-Path "$PSScriptRoot\..\.."
$version = (Select-String -Path "$native\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

# O Opus compila em C e pede o CMake, que o Build Tools traz fora do PATH.
$cmake = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
if (Test-Path $cmake) { $env:Path = "$cmake;$env:Path" }

cargo build --release -p unkvoid-windows --manifest-path "$native\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "o app nao compilou" }

# O NSIS que o Tauri baixa na primeira build dele.
$makensis = "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe"
if (-not (Test-Path $makensis)) { throw "makensis nao encontrado em $makensis" }

$out = "$native\target\release\bundle\windows"
New-Item -ItemType Directory -Force $out | Out-Null
$installer = "$out\Unkvoid_${version}_x64-setup.exe"

& $makensis /V2 /INPUTCHARSET UTF8 "/DVERSION=$version" "/DBINARY=$native\target\release\unkvoid.exe" `
    "/DICON=$native\apps\desktop\src-tauri\icons\icon.ico" "/DOUTFILE=$installer" "$PSScriptRoot\installer.nsi"
if ($LASTEXITCODE -ne 0) { throw "o instalador nao empacotou" }

Write-Output "[INFO] $installer"
