#!/usr/bin/env pwsh
# ============================================================================
# Mustard — instalador para teste (Windows)
#
# Instala os binários pré-compilados do Mustard (scan, mustard-rt, mustard)
# + o rtk empacotado. NÃO precisa do toolchain Rust — são binários já
# compilados, e tudo o que o `mustard init` grava vem dentro deles.
#
# Layout após instalar (auto-contido, fácil de remover):
#   %USERPROFILE%\.mustard\bin\        -> entra no PATH (mustard, …, rtk)
#
# Uso:
#   .\install.ps1                       # instala binários + ajusta PATH (sem init)
#   .\install.ps1 -Target C:\proj       # também roda `mustard init` nesse projeto
#   .\install.ps1 -Target C:\proj -Force  # sobrescreve um .claude/ existente
# ============================================================================
[CmdletBinding()]
param(
    [string]$Target,
    [switch]$Force
)
$ErrorActionPreference = 'Stop'

$ScriptDir = $PSScriptRoot
$PkgBin    = Join-Path $ScriptDir 'bin'
if (-not (Test-Path $PkgBin)) { throw "bin\ não encontrado em $PkgBin — rode de dentro do pacote descompactado." }

$Prefix = if ($env:MUSTARD_PREFIX) { $env:MUSTARD_PREFIX } else { Join-Path $env:USERPROFILE '.mustard' }
$BinDir = Join-Path $Prefix 'bin'

Write-Host "==> Instalando o Mustard em $Prefix"
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
Copy-Item -Path (Join-Path $PkgBin '*') -Destination $BinDir -Recurse -Force

# A pasta de moldes que versões anteriores deste instalador copiavam ao lado de
# bin\: nenhum programa a lê mais, e ela sai aqui, só nesse lugar.
$OldTemplates = Join-Path $Prefix 'templates'
if (Test-Path $OldTemplates) { Remove-Item -Recurse -Force $OldTemplates }

# PATH da sessão atual, para o init abaixo enxergar mustard + rtk.
$env:PATH = "$BinDir;$env:PATH"

# --- garante o rtk ----------------------------------------------------------
if (Get-Command rtk -ErrorAction SilentlyContinue) {
    Write-Host "  rtk presente: $((Get-Command rtk).Source)"
} elseif (Test-Path (Join-Path $BinDir 'rtk.exe')) {
    Write-Host "  rtk instalado a partir do pacote: $(Join-Path $BinDir 'rtk.exe')"
} else {
    Write-Warning "  rtk não encontrado e não veio no pacote. Instale manualmente:"
    Write-Warning "    scoop install rtk   (ou)   cargo install --git https://github.com/rtk-ai/rtk"
}

# --- persiste o PATH (escopo User, sem truncar) -----------------------------
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not (($userPath -split ';') -contains $BinDir)) {
    $newPath = if ([string]::IsNullOrEmpty($userPath)) { $BinDir } else { "$userPath;$BinDir" }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "  $BinDir adicionado ao seu PATH de usuário (reinicie os terminais para valer)."
}

# --- opcional: prepara um projeto ------------------------------------------
if ($Target) {
    $resolved = Resolve-Path -LiteralPath $Target -ErrorAction SilentlyContinue
    if (-not $resolved) { throw "projeto-alvo não existe: $Target" }
    $Target = $resolved.Path
    Write-Host "==> Rodando 'mustard init' em $Target"
    $initArgs = @('init', '--yes')
    if ($Force) { $initArgs += '--force' }
    Push-Location $Target
    try {
        & (Join-Path $BinDir 'mustard.exe') @initArgs
        if ($LASTEXITCODE -ne 0) { throw "mustard init falhou (exit $LASTEXITCODE)." }
    } finally { Pop-Location }
} else {
    Write-Host "==> Binários instalados. Para preparar um projeto:  cd <projeto>; mustard init"
}

Write-Host "==> Pronto. Abra um NOVO terminal para o 'mustard' entrar no PATH."
