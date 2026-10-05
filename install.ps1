#!/usr/bin/env pwsh
# install.ps1 — Build + install Mustard and scaffold .claude/ into a project.
#
# Dogfooding installer: it builds the binaries (scan, mustard-rt, and mustard)
# in release with ONE `cargo build --release --locked` call (the locked
# dependency versions, like the session build), copies them to ~/.cargo/bin (so
# the hooks in .claude/settings.json — which invoke `mustard-rt` from PATH —
# resolve at runtime, and `mustard-rt` finds the `scan` miner as a
# ~/.cargo/bin sibling), then runs `mustard init` in the target project.
# Everything init lays down is compiled into the binary, so it needs no folder
# beside it.
#
# Usage:
#   .\install.ps1                  # prompt for the target (default CWD), then `mustard init`
#   .\install.ps1 -Target ..\app   # scaffold (new) OR refresh (existing); `mustard init` is idempotent
#   .\install.ps1 -Force           # overwrite an existing .claude/ (no backup)
#   .\install.ps1 -DryRun          # show init actions without writing
#   .\install.ps1 -SkipBuild       # skip the build and the copy (binaries already installed)
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
$ScanExe      = Join-Path $CargoBin 'scan.exe'
$BuiltDir     = Join-Path $Root 'target\install\release'
$BuildNumFile = Join-Path $Root '.mustard-build-number'

# Native commands don't throw on a non-zero exit under $ErrorActionPreference;
# check $LASTEXITCODE explicitly so a failed build/init aborts the installer.
function Assert-LastExit([string]$What) {
    if ($LASTEXITCODE -ne 0) { throw "$What failed (exit $LASTEXITCODE)." }
}

# Bump the gitignored per-build counter and return the new value. The cargo
# build's build.rs stamps this into `mustard --version` / `mustard-rt --version`
# as MUSTARD_BUILD_NUMBER. The file is created with 1 on first build; a missing
# or garbled value resets to 1 rather than aborting the install.
function Step-BuildNumber([string]$Path) {
    $current = 0
    if (Test-Path -LiteralPath $Path) {
        $raw = (Get-Content -LiteralPath $Path -Raw -ErrorAction SilentlyContinue).Trim()
        [int]::TryParse($raw, [ref]$current) | Out-Null
    }
    $next = $current + 1
    Set-Content -LiteralPath $Path -Value $next -NoNewline -Encoding utf8
    return $next
}

# Replace an installed binary with the freshly built one, tolerating the Windows
# lock on a running .exe. Any live hook holds ~/.cargo/bin/mustard-rt.exe open
# for the whole Claude Code session, so overwriting it fails with "Access is
# denied (os error 5)" — a binary mapped into a running process cannot be
# overwritten. Windows DOES allow *renaming* that binary, though: the running
# image keeps its handle on the renamed file while the original name is freed
# for the fresh copy. So park the in-use binary aside first; the old image stays
# valid for the holding processes until they exit (next Claude Code restart).
function Install-Bin([string]$BuiltExe, [string]$ExePath) {
    if (-not (Test-Path -LiteralPath $BuiltExe)) { throw "The build did not produce $BuiltExe." }
    $parked = $null
    if (Test-Path $ExePath) {
        # Best-effort sweep of stale parks from earlier installs whose holders
        # have since exited; a still-locked .old- is skipped silently.
        $dir  = Split-Path -Parent $ExePath
        $leaf = Split-Path -Leaf   $ExePath
        Get-ChildItem -LiteralPath $dir -Filter "$leaf.old-*" -ErrorAction SilentlyContinue |
            ForEach-Object { try { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction Stop } catch {} }
        # Free the name. Rename (not delete/overwrite) succeeds even while the
        # image is mapped into a running process.
        $parked = "$ExePath.old-$([guid]::NewGuid().ToString('N').Substring(0,8))"
        try { Move-Item -LiteralPath $ExePath -Destination $parked -Force -ErrorAction Stop }
        catch { throw "Could not free $ExePath for replacement: $($_.Exception.Message). Close running mustard-rt processes (hooks) and re-run." }
    }
    try { Copy-Item -LiteralPath $BuiltExe -Destination $ExePath -Force -ErrorAction Stop }
    catch {
        # Copy failed: restore the previous binary so the install isn't left
        # without one.
        if ($parked -and (Test-Path $parked) -and -not (Test-Path $ExePath)) {
            Move-Item -LiteralPath $parked -Destination $ExePath -Force -ErrorAction SilentlyContinue
        }
        throw "Could not copy $BuiltExe to ${ExePath}: $($_.Exception.Message)"
    }
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
    # Bump the per-build counter and feed it to the cargo build as
    # MUSTARD_BUILD_NUMBER (the build.rs in apps/rt + apps/cli stamps it into
    # `--version`). Scope the env var to the build invocation and restore it
    # afterwards, so the script stays safe to dot-source.
    $buildNumber       = Step-BuildNumber $BuildNumFile
    $prevBuildNumber   = $env:MUSTARD_BUILD_NUMBER
    $env:MUSTARD_BUILD_NUMBER = $buildNumber
    # Keep ONE build cache across re-runs: the first run pays the cold build
    # once and every later run only recompiles what changed — plus apps/rt and
    # apps/cli, whose build.rs re-stamps the bumped build number
    # (`rerun-if-env-changed=MUSTARD_BUILD_NUMBER`); that residual is the price
    # of a truthful `--version` and is seconds, not minutes. `target\install`
    # keeps the release cache apart from the workspace's own dev `target\`, and
    # lives under the already-gitignored target/ tree.
    $prevTargetDir     = $env:CARGO_TARGET_DIR
    $env:CARGO_TARGET_DIR = Join-Path $Root 'target\install'
    Write-Host "==> Building scan + mustard-rt + mustard (release, locked)  (build #$buildNumber)"
    Write-Host "    CARGO_TARGET_DIR=$env:CARGO_TARGET_DIR (shared cache — later runs are incremental)"
    try {
        Push-Location $Root
        try {
            cargo build --release --locked -p scan -p mustard-rt -p mustard-cli
            Assert-LastExit 'cargo build'
        } finally {
            Pop-Location
        }
    } finally {
        $env:MUSTARD_BUILD_NUMBER = $prevBuildNumber
        $env:CARGO_TARGET_DIR     = $prevTargetDir
    }
    Write-Host "==> Copying the binaries to $CargoBin ..."
    if (-not (Test-Path $CargoBin)) { New-Item -ItemType Directory -Path $CargoBin -Force | Out-Null }
    # scan first: mustard-rt resolves it as a ~/.cargo/bin sibling at runtime
    # (Scan::locate), and the facts projection depends on it.
    Install-Bin (Join-Path $BuiltDir 'scan.exe')       $ScanExe
    Install-Bin (Join-Path $BuiltDir 'mustard-rt.exe') $RtExe
    Install-Bin (Join-Path $BuiltDir 'mustard.exe')    $MustardExe
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

# `mustard init` is idempotent (Mustard 2.0): the content payload ships in the
# plugin, so init only seeds the small harness files (settings.json seed +
# plugin-enable, CLAUDE.md, .gitignore) and re-stamps the version. Re-running it
# on an installed project is the safe refresh -- the job the retired
# `mustard update` used to do. So init handles both the fresh and existing case.
# -Force overwrites .claude/ without a backup; -DryRun previews.
$cmdArgs = @('init', '--yes')
if ($Force)  { $cmdArgs += '--force' }
if ($DryRun) { $cmdArgs += '--dry-run' }
$cmdLabel = 'mustard init'

Write-Host "==> mustard $($cmdArgs -join ' ')   (target: $Target)"
Push-Location $Target
try {
    & $MustardExe @cmdArgs
    Assert-LastExit $cmdLabel
} finally {
    Pop-Location
}
Write-Host '==> Done. .claude/ is installed; mustard-rt hooks are wired via settings.json.'

# A live hook keeps the *previous* binary mapped until it exits. The fresh build
# is already on disk, but running processes will not pick it up until they
# restart.
if (-not $SkipBuild) {
    $stillRunning = @(Get-Process -Name mustard-rt -ErrorAction SilentlyContinue)
    if ($stillRunning.Count -gt 0) {
        Write-Warning "$($stillRunning.Count) mustard-rt process(es) are still running the PREVIOUS binary."
        Write-Host   '  - Restart Claude Code to pick up the freshly-installed binary.'
    }
}
