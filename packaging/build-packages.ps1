#!/usr/bin/env pwsh
# ============================================================================
# build-packages.ps1 — empacota o Mustard para distribuição.
#
# Windows (SEM dashboard): pacote auto-contido de binários pré-compilados, sem
# precisar do toolchain:
#   dist/mustard-windows-x64.zip       (binários .exe MSVC, compilados aqui)
#
# Linux (COM dashboard — instalação completa): um único pacote Debian que traz
# os binários do CLI E o Mustard Dashboard (app Tauri), compilados num Docker
# Ubuntu 22.04 (glibc 2.35 -> roda em Ubuntu 22.04+; o webkit2gtk-4.1 do Tauri 2
# não existe no 20.04):
#   dist/mustard_<versao>_amd64.deb    + install.sh (apt) + TUTORIAL-LINUX.md
#
# O pacote Windows contém: bin/ (scan, mustard-rt, mustard-mcp, mustard, rtk),
# templates/, install.ps1 e README.txt. O .deb Linux instala tudo via `apt`
# (que resolve as dependências de sistema do dashboard sozinho) — ver
# packaging/linux/Dockerfile + packaging/linux/build-deb.sh.
#
# Uso:
#   .\packaging\build-packages.ps1                 # windows + linux
#   .\packaging\build-packages.ps1 -Targets windows
#   .\packaging\build-packages.ps1 -Targets linux
# ============================================================================
[CmdletBinding()]
param(
    [ValidateSet('windows', 'linux', 'both')][string]$Targets = 'both'
)
$ErrorActionPreference = 'Stop'

$PkgDir       = $PSScriptRoot
$Root         = Split-Path -Parent $PkgDir
$Installer    = Join-Path $PkgDir 'installer'
$Dist         = Join-Path $Root 'dist'
$Stage        = Join-Path $Dist '_stage'
$TemplatesSrc = Join-Path $Root 'apps\cli\templates'
$Bins         = @('scan', 'mustard-rt', 'mustard-mcp', 'mustard')

function New-CleanDir([string]$p) {
    if (Test-Path $p) { Remove-Item -Recurse -Force $p }
    New-Item -ItemType Directory -Force -Path $p | Out-Null
}

if (-not (Test-Path $TemplatesSrc)) { throw "templates payload não encontrado em $TemplatesSrc — rode da raiz do repo." }
if (-not (Test-Path $Installer))    { throw "instaladores não encontrados em $Installer." }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

# ---------------------------------------------------------------- Windows ----
if ($Targets -in 'windows', 'both') {
    Write-Host "==> [windows] cargo build --release (4 binários)"
    Push-Location $Root
    try {
        cargo build --release --bin scan --bin mustard-rt --bin mustard-mcp --bin mustard
        if ($LASTEXITCODE -ne 0) { throw "cargo build (windows) falhou (exit $LASTEXITCODE)." }
    } finally { Pop-Location }

    $pkg = Join-Path $Stage 'mustard-windows-x64'
    New-CleanDir (Join-Path $pkg 'bin')
    foreach ($b in $Bins) {
        $src = Join-Path $Root "target\release\$b.exe"
        if (-not (Test-Path $src)) { throw "binário Windows ausente: $src" }
        Copy-Item $src (Join-Path $pkg 'bin') -Force
    }
    # rtk empacotado (best-effort, a partir do que estiver no PATH desta máquina)
    $rtk = (Get-Command rtk -ErrorAction SilentlyContinue).Source
    if ($rtk) {
        Copy-Item $rtk (Join-Path $pkg 'bin\rtk.exe') -Force
        Write-Host "  rtk empacotado: $rtk"
    } else {
        Write-Warning "  rtk não está no PATH — pacote Windows vai sem rtk (o instalador instrui)."
    }
    Copy-Item $TemplatesSrc (Join-Path $pkg 'templates') -Recurse -Force
    Copy-Item (Join-Path $Installer 'install.ps1') $pkg -Force
    Copy-Item (Join-Path $Installer 'README.txt')  $pkg -Force

    $zip = Join-Path $Dist 'mustard-windows-x64.zip'
    if (Test-Path $zip) { Remove-Item -Force $zip }
    Compress-Archive -Path $pkg -DestinationPath $zip
    Write-Host "==> gravado $zip"
}

# ------------------------------------------------------------------ Linux ----
if ($Targets -in 'linux', 'both') {
    if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
        throw "docker não encontrado — necessário para o build Linux."
    }

    # A imagem traz Rust + Node + as dependências de build do Tauri (webkit etc.)
    # e o ferramental .deb. É cacheada por camadas do Docker — só a 1ª vez é
    # demorada. O build em si (CLI + dashboard + fusão no .deb) roda no container
    # via packaging/linux/build-deb.sh, lendo o repo montado em /work.
    $img        = 'mustard-linux-builder'
    $linuxCtx   = Join-Path $PkgDir 'linux'
    Write-Host "==> [linux] docker build $img  (Ubuntu 22.04 + Rust + Node + Tauri/webkit)"
    docker build -t $img $linuxCtx
    if ($LASTEXITCODE -ne 0) { throw "docker build da imagem Linux falhou (exit $LASTEXITCODE)." }

    # Volumes nomeados cacheiam registry/target/pnpm entre execuções (re-empacotar
    # fica rápido). Limpe com:
    #   docker volume rm mustard-deb-cargo-registry mustard-deb-cargo-git `
    #     mustard-deb-cli-target mustard-deb-dash-target mustard-deb-pnpm
    Write-Host "==> [linux] docker run — compila CLI + dashboard e funde no .deb (pode levar vários minutos)"
    docker run --rm `
        -v "mustard-deb-cargo-registry:/opt/cargo/registry" `
        -v "mustard-deb-cargo-git:/opt/cargo/git" `
        -v "mustard-deb-cli-target:/tmp/cli-target" `
        -v "mustard-deb-dash-target:/tmp/dash-target" `
        -v "mustard-deb-pnpm:/tmp/pnpm-store" `
        -v "${Root}:/work" `
        -v "${Dist}:/dist" `
        -w /work `
        $img `
        bash /work/packaging/linux/build-deb.sh
    if ($LASTEXITCODE -ne 0) { throw "build Linux no Docker falhou (exit $LASTEXITCODE)." }

    $deb = Get-ChildItem $Dist -Filter 'mustard_*_amd64.deb' -File |
        Sort-Object LastWriteTime | Select-Object -Last 1
    if (-not $deb) { throw "build Linux não gerou o .deb em $Dist." }
    Write-Host "==> gravado $($deb.FullName)"
}

Write-Host ""
Write-Host "==> Pacotes em $Dist :"
Get-ChildItem $Dist -Filter 'mustard*' -File |
    Where-Object { $_.Extension -in '.zip', '.deb', '.gz' } | ForEach-Object {
        Write-Host ("    {0}  ({1:N1} MB)" -f $_.Name, ($_.Length / 1MB))
    }
