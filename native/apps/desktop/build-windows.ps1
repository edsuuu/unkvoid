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
  # A chave mora no WSL, junto com os outros segredos, e e lida de la. A copia que
  # existia em %USERPROFILE%\.tauri era de um par antigo: assinava sem erro, e o app
  # recusava a assinatura em execucao porque o pubkey embutido e de outro par.
  [string]$Key  = "\\wsl.localhost\Ubuntu-26.04\home\edsu\auxilos\unkvoid.key"
)
$ErrorActionPreference = "Stop"
function Assert-LastExit($msg) { if ($LASTEXITCODE -ne 0) { throw "$msg (codigo $LASTEXITCODE)" } }

# O identificador do par de chaves, tanto da publica do tauri.conf.json quanto de uma
# assinatura. Sao o mesmo formato por dentro: dois bytes de algoritmo e oito de id.
function Get-KeyId([string]$base64) {
  $texto = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($base64.Trim()))
  $corpo = @($texto -split "`n" | Where-Object { $_.Trim() -and $_ -notmatch '^(un)?trusted comment:' })
  return [BitConverter]::ToString([Convert]::FromBase64String($corpo[0].Trim())[2..9])
}

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
robocopy $Src $Work /MIR /XD node_modules vendor target .git .claude dist .idea .vscode /XF .git /NFL /NDL /NJH /NJS /NP | Out-Null
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

# 5. Build, com o ambiente montado a mao.
#
#    A senha da chave e vazia, e `$env:VAR = ""` no PowerShell nao guarda uma string
#    vazia: APAGA a variavel. O processo filho nascia sem a senha, o Tauri parava
#    pedindo ela num terminal que ninguem esta olhando, e o build ficava pendurado com
#    os dois instaladores ja prontos e nenhum .sig ao lado. O bloco de ambiente do
#    ProcessStartInfo aceita valor vazio, que e o que o `bash` sempre fez no Mac.
Write-Host "==> Compilando .msi e .exe (a primeira vez demora)" -ForegroundColor Cyan
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = "cmd.exe"
$psi.Arguments = "/c npx tauri build --bundles nsis,msi"
$psi.WorkingDirectory = (Get-Location).Path
$psi.UseShellExecute = $false
$psi.EnvironmentVariables["TAURI_SIGNING_PRIVATE_KEY"] = (Get-Content -Raw $Key)
$psi.EnvironmentVariables["TAURI_SIGNING_PRIVATE_KEY_PASSWORD"] = ""
$build = [System.Diagnostics.Process]::Start($psi)
$build.WaitForExit()
if ($build.ExitCode -ne 0) { throw "tauri build falhou (codigo $($build.ExitCode))" }

# 6. As duas conferencias que nao dao pra pular, no que o build acabou de produzir -
#    e nao na pasta de saida, que guarda build de outras versoes.
#
#    Um instalador sem .sig ao lado sobe sem erro nenhum e some do manifesto depois,
#    longe daqui. E uma assinatura de OUTRO par de chaves e pior: o build passa com um
#    aviso no meio de mil linhas, a publicacao parece certa, e a recusa so acontece na
#    maquina de quem instalou - onde ninguem esta olhando. Foi assim que a 0.0.7 foi
#    publicada assinada com uma chave que o app nao reconhece.
$bundle = "$Work\native\target\release\bundle"
$conf = Get-Content -Raw (Join-Path $Work "native\apps\desktop\src-tauri\tauri.conf.json") | ConvertFrom-Json
$esperado = Get-KeyId $conf.plugins.updater.pubkey

$installers = Get-ChildItem "$bundle\msi\*.msi", "$bundle\nsis\*-setup.exe"
foreach ($file in $installers) {
  $sig = "$($file.FullName).sig"
  if (-not (Test-Path $sig)) { throw "$($file.Name) saiu sem .sig - ninguem se atualiza para ele" }

  $assinou = Get-KeyId (Get-Content -Raw $sig)
  if ($assinou -ne $esperado) {
    throw "$($file.Name) foi assinado pela chave $assinou, e o app so aceita a $esperado. Confira o -Key."
  }
}

# 7. Copia o resultado, cada instalador com a assinatura junto
New-Item -ItemType Directory -Force $Out | Out-Null
foreach ($file in $installers) { Copy-Item $file.FullName, "$($file.FullName).sig" $Out -Force }

Write-Host "`n==> Pronto, assinado pela chave $esperado. Arquivos em $Out :" -ForegroundColor Green
$installers | Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,1)}}
Write-Host "`nPara publicar, no WSL: make publish-windows" -ForegroundColor Cyan
