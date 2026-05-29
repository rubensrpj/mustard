#!/usr/bin/env pwsh
# install.ps1 — Build + install Mustard and scaffold .claude/ into a project.
#
# Dogfooding installer: it builds the two binaries in release, installs them to
# ~/.cargo/bin (so the hooks in .claude/settings.json — which invoke `mustard-rt`
# from PATH — resolve at runtime), then runs `mustard init` in the target
# project, pointed at this repo's bundled templates/ payload.
#
# Why MUSTARD_TEMPLATES_DIR: `cargo install` copies only the binary to
# ~/.cargo/bin, not its templates/ payload. Without an explicit pointer the
# installed `mustard` would fall back to the compile-time CARGO_MANIFEST_DIR
# path, which silently breaks if this repo is ever moved. We set the env var to
# apps/cli/templates for the init invocation so it always resolves the payload
# that ships with the binaries we just built.
#
# Usage:
#   .\install.ps1                  # prompt for the target (default CWD), then `mustard init`
#   .\install.ps1 -Target ..\app   # scaffold into another project (no prompt)
#   .\install.ps1 -Force           # overwrite an existing .claude/ (no backup)
#   .\install.ps1 -DryRun          # show init actions without writing
#   .\install.ps1 -SkipBuild       # skip cargo install (binaries already installed)
[CmdletBinding()]
param(
    [string]$Target = (Get-Location).Path,
    [switch]$Force,
    [switch]$DryRun,
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root         = $PSScriptRoot
$CargoBin     = Join-Path $env:USERPROFILE '.cargo\bin'
$MustardExe   = Join-Path $CargoBin 'mustard.exe'
$RtExe        = Join-Path $CargoBin 'mustard-rt.exe'
$TemplatesDir = Join-Path $Root 'apps\cli\templates'

# Native commands don't throw on a non-zero exit under $ErrorActionPreference;
# check $LASTEXITCODE explicitly so a failed build/init aborts the installer.
function Assert-LastExit([string]$What) {
    if ($LASTEXITCODE -ne 0) { throw "$What failed (exit $LASTEXITCODE)." }
}

# Prerequisite: the bundled templates/ payload `mustard init` copies from.
if (-not (Test-Path $TemplatesDir)) {
    throw "Templates payload not found at $TemplatesDir — run this script from the Mustard repo root."
}

# Resolve the target project — the directory `mustard init` scaffolds .claude/
# into. Defaults to the CWD; pass -Target to script it, or accept the prompt
# when running interactively without -Target. The directory must already exist
# (init scaffolds into an existing project, it does not create one).
if (-not $PSBoundParameters.ContainsKey('Target') -and
    [Environment]::UserInteractive -and -not [Console]::IsInputRedirected) {
    $entered = Read-Host "Target project for .claude/ (Enter to use $Target)"
    if (-not [string]::IsNullOrWhiteSpace($entered)) { $Target = $entered.Trim() }
}
$resolved = Resolve-Path -LiteralPath $Target -ErrorAction SilentlyContinue
if (-not $resolved) {
    throw "Target directory does not exist: $Target — create it first, or pass an existing project path."
}
$Target = $resolved.Path

if (-not $SkipBuild) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'cargo is not on PATH. Install the Rust toolchain (https://rustup.rs) and re-run.'
    }
    Write-Host '==> Installing mustard-rt + mustard (release) to ~/.cargo/bin ...'
    cargo install --path (Join-Path $Root 'apps\rt')  --bin mustard-rt --force
    Assert-LastExit 'cargo install mustard-rt'
    cargo install --path (Join-Path $Root 'apps\cli') --bin mustard    --force
    Assert-LastExit 'cargo install mustard'
}
if (-not (Test-Path $MustardExe)) { $MustardExe = 'mustard' }  # fall back to PATH

# The hooks wired into .claude/settings.json call `mustard-rt` and `rtk` from
# PATH at Claude Code runtime. Surface now — before init — anything that would
# leave the installed .claude/ unable to run its hooks.
$pathDirs       = ($env:PATH -split ';' | ForEach-Object { $_.TrimEnd('\') })
$cargoBinOnPath = $pathDirs -contains $CargoBin.TrimEnd('\')
if ((Test-Path $RtExe) -and -not $cargoBinOnPath) {
    Write-Warning "mustard-rt is installed but $CargoBin is not on PATH; the .claude/ hooks will not resolve at runtime."
    Write-Warning "  Add it persistently:  setx PATH `"$CargoBin;`$env:PATH`"   (then restart your shell)"
} elseif (-not (Test-Path $RtExe) -and -not (Get-Command mustard-rt -ErrorAction SilentlyContinue)) {
    Write-Warning 'mustard-rt was not found. Re-run without -SkipBuild, or ensure it is on PATH so the hooks resolve.'
}

# RTK is a hard dependency of `mustard init` (it probes `rtk --version` and
# aborts if missing). Warn early with install instructions; init is the
# authority and will refuse to run without it (except under -DryRun).
if (-not $DryRun -and -not (Get-Command rtk -ErrorAction SilentlyContinue)) {
    Write-Warning 'rtk (Rust Token Killer) is not on PATH; `mustard init` requires it and will abort.'
    Write-Warning '  Windows: scoop install rtk   (or)   cargo install --git https://github.com/rtk-ai/rtk'
}

$initArgs = @('init', '--yes')
if ($Force)  { $initArgs += '--force' }
if ($DryRun) { $initArgs += '--dry-run' }

# Point init at this repo's templates/ payload (see header). Scope the env var
# to the init child process and restore it afterwards so the script is safe to
# dot-source.
$prevTemplates = $env:MUSTARD_TEMPLATES_DIR
$env:MUSTARD_TEMPLATES_DIR = $TemplatesDir

Write-Host "==> mustard $($initArgs -join ' ')   (target: $Target)"
Write-Host "    MUSTARD_TEMPLATES_DIR=$TemplatesDir"
Push-Location $Target
try {
    & $MustardExe @initArgs
    Assert-LastExit 'mustard init'
} finally {
    Pop-Location
    $env:MUSTARD_TEMPLATES_DIR = $prevTemplates
}
Write-Host '==> Done. .claude/ is installed; mustard-rt hooks are wired via settings.json.'
