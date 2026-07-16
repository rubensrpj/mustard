#!/usr/bin/env pwsh
# install.ps1 — Build + install Mustard and scaffold .claude/ into a project.
#
# Dogfooding installer: it builds the binaries (scan, mustard-translate,
# mustard-rt, mustard-mcp, and mustard)
# in release, installs them to ~/.cargo/bin (so the hooks in .claude/settings.json
# — which invoke `mustard-rt` from PATH — resolve at runtime, and `mustard-rt`
# finds the `scan` miner AND the `mustard-translate` sidecar as ~/.cargo/bin
# siblings), then runs `mustard init`
# in the target project, pointed at this repo's bundled templates/ payload.
#
# `mustard-translate` (apps/translate, outside the workspace) is the LOCAL MT
# sidecar the retrieval uses for the automatic gloss + `scan-equivalences`;
# both are fail-open, so skipping it degrades retrieval silently — that is why
# the installer ships it alongside the core four.
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
#   .\install.ps1 -Target ..\app   # scaffold (new) OR refresh templates (existing); `mustard init` is idempotent
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
$McpExe       = Join-Path $CargoBin 'mustard-mcp.exe'
$ScanExe      = Join-Path $CargoBin 'scan.exe'
$TranslateExe = Join-Path $CargoBin 'mustard-translate.exe'
$TemplatesDir = Join-Path $Root 'apps\cli\templates'
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

# Build a crate and replace its installed binary, tolerating the Windows lock on
# a running .exe. The mustard-rt MCP server (`mustard-rt mcp`) and any live hook
# hold ~/.cargo/bin/mustard-rt.exe open for the whole Claude Code session, so
# `cargo install --force` fails its final move with "Access is denied (os error
# 5)" — it cannot overwrite a binary that is mapped into a running process.
# Windows DOES allow *renaming* that binary, though: the running image keeps its
# handle on the renamed file while the original name is freed for cargo to write
# the fresh build. So park the in-use binary aside first; the old image stays
# valid for the holding processes until they exit (next Claude Code restart).
function Install-Bin([string]$ExePath, [string]$CratePath, [string]$BinName) {
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
        catch { throw "Could not free $ExePath for replacement: $($_.Exception.Message). Close running mustard-rt processes (MCP servers / hooks) and re-run." }
    }
    cargo install --path $CratePath --bin $BinName --force
    if ($LASTEXITCODE -ne 0) {
        # Build failed: restore the previous binary so the install isn't left
        # without one (cargo only writes the new exe after a successful build).
        if ($parked -and (Test-Path $parked) -and -not (Test-Path $ExePath)) {
            Move-Item -LiteralPath $parked -Destination $ExePath -Force -ErrorAction SilentlyContinue
        }
        throw "cargo install $BinName failed (exit $LASTEXITCODE)."
    }
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
    # Bump the per-build counter and feed it to the cargo build as
    # MUSTARD_BUILD_NUMBER (the build.rs in apps/rt + apps/cli stamps it into
    # `--version`). Scope the env var to the two build invocations and restore
    # it afterwards, exactly like MUSTARD_TEMPLATES_DIR below, so the script
    # stays safe to dot-source.
    $buildNumber       = Step-BuildNumber $BuildNumFile
    $prevBuildNumber   = $env:MUSTARD_BUILD_NUMBER
    $env:MUSTARD_BUILD_NUMBER = $buildNumber
    Write-Host "==> Installing scan + mustard-translate + mustard-rt + mustard-mcp + mustard (release) to ~/.cargo/bin ...  (build #$buildNumber)"
    try {
        # scan first: mustard-rt resolves it as a ~/.cargo/bin sibling at runtime
        # (Scan::locate), and the feature/spec/digest/facts flow depends on it.
        # mustard-translate next: the retrieval's local-MT sidecar (gloss +
        # scan-equivalences), resolved the same sibling-first way.
        Install-Bin $ScanExe      (Join-Path $Root 'apps\scan')      'scan'
        Install-Bin $TranslateExe (Join-Path $Root 'apps\translate') 'mustard-translate'
        Install-Bin $RtExe        (Join-Path $Root 'apps\rt')        'mustard-rt'
        Install-Bin $McpExe       (Join-Path $Root 'apps\mcp')       'mustard-mcp'
        Install-Bin $MustardExe   (Join-Path $Root 'apps\cli')       'mustard'
    } finally {
        $env:MUSTARD_BUILD_NUMBER = $prevBuildNumber
    }
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
}

# Point the command at this repo's templates/ payload (init resolves it
# via MUSTARD_TEMPLATES_DIR). Scope the env var to the child process
# and restore it afterwards so the script is safe to dot-source.
$prevTemplates = $env:MUSTARD_TEMPLATES_DIR
$env:MUSTARD_TEMPLATES_DIR = $TemplatesDir

Write-Host "==> mustard $($cmdArgs -join ' ')   (target: $Target)"
Write-Host "    MUSTARD_TEMPLATES_DIR=$TemplatesDir"
Push-Location $Target
try {
    & $MustardExe @cmdArgs
    Assert-LastExit $cmdLabel
} finally {
    Pop-Location
    $env:MUSTARD_TEMPLATES_DIR = $prevTemplates
}
Write-Host '==> Done. .claude/ is installed; mustard-rt hooks are wired via settings.json.'

# A long-running `mustard-rt mcp` server (the mustard-memory MCP face) and the
# OTEL collector daemon keep the *previous* binary mapped until they exit. The
# fresh build is already on disk, but live processes won't pick it up until they
# restart. The OTEL collector is the worst offender on Windows: it holds an
# exclusive lock on `mustard-rt.exe`, which can strand the *next* build. So stop
# it now via the freshly-installed binary, then surface what the user must do.
if (-not $SkipBuild) {
    # Best-effort teardown of the OTEL collector via the new binary. This runs
    # under $ErrorActionPreference='Stop', so wrap it so any failure (missing
    # exe, kill error, no listener) can NEVER abort the install.
    if (Test-Path $RtExe) {
        try {
            & $RtExe run otel-stop
        } catch {
            # Fail-open: teardown is advisory; an install must not hinge on it.
        }
    }

    $stillRunning = @(Get-Process -Name mustard-rt -ErrorAction SilentlyContinue)
    if ($stillRunning.Count -gt 0) {
        Write-Warning "$($stillRunning.Count) mustard-rt process(es) are still running the PREVIOUS binary."
        Write-Host   '  - The OTEL collector was just stopped; it respawns automatically on the next Claude Code session, picking up the fresh binary.'
        Write-Host   '  - The MCP server (`mustard-rt mcp`) can be refreshed IN-SESSION without a full Claude Code restart:'
        Write-Host   '      open the /mcp panel -> select `mustard-memory` -> Reconnect.'
        Write-Host   '    Reconnect re-executes the command from disk, so it picks up the freshly-installed binary.'
        Write-Host   '  - A full Claude Code restart also works, if you prefer it.'
    }
}
