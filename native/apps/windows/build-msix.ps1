# Empacota o app nativo do Windows como MSIX, para a Microsoft Store.
#
# O .msix sai sem assinatura: quem assina é a Store, depois de aprovar o envio no Partner
# Center. Para instalar este arquivo fora da Store seria preciso assiná-lo com um
# certificado em que a máquina confia.
#
#   powershell -ExecutionPolicy Bypass -File native\apps\windows\build-msix.ps1
$ErrorActionPreference = 'Stop'

$native = Resolve-Path "$PSScriptRoot\..\.."
$version = (Select-String -Path "$native\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
# A Store quer quatro números, sem sufixo, e o quarto é dela: sempre 0.
$packageVersion = ($version -replace '-.*$', '') + '.0'

# O Opus compila em C e pede o CMake, que o Build Tools traz fora do PATH.
$cmake = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin"
if (Test-Path $cmake) { $env:Path = "$cmake;$env:Path" }

cargo build --release -p unkvoid-windows --manifest-path "$native\Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "o app nao compilou" }

$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" | Sort-Object FullName | Select-Object -Last 1
if (-not $makeappx) { throw "makeappx nao encontrado: instale o Windows SDK" }

$layout = "$native\target\release\msix"
if (Test-Path $layout) { Remove-Item -Recurse -Force $layout }
New-Item -ItemType Directory -Force "$layout\Assets" | Out-Null

Copy-Item "$native\target\release\unkvoid.exe" $layout
foreach ($asset in 'Square44x44Logo.png', 'Square150x150Logo.png', 'StoreLogo.png') {
    Copy-Item "$native\apps\desktop\src-tauri\icons\$asset" "$layout\Assets\"
}

$manifest = (Get-Content -Raw -Encoding UTF8 "$PSScriptRoot\msix\AppxManifest.xml").Replace('{VERSION}', $packageVersion)
[System.IO.File]::WriteAllText("$layout\AppxManifest.xml", $manifest, (New-Object System.Text.UTF8Encoding $false))

$out = "$native\target\release\bundle\windows"
New-Item -ItemType Directory -Force $out | Out-Null
$package = "$out\Unkvoid_${packageVersion}_x64.msix"

& $makeappx.FullName pack /o /d $layout /p $package
if ($LASTEXITCODE -ne 0) { throw "o pacote nao montou" }

Write-Output "[INFO] $package"
