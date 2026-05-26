# cleanup-legacy-claude.ps1 -- one-shot purge of legacy `.claude/` artefacts.
#
# Purpose
#
# Wave 5 of `2026-05-26-w2-residuals-50-unlisted-apps-rt` (AC-W5.3 / AC-G3
# umbrella) requires the repo's `.claude/` to no longer host the legacy
# volatile artefacts listed below. They have all moved into their new homes
# (`.claude/.cache/`, per-spec directories, etc.) but historical runs may have
# left the old shells behind.
#
# Idempotent: every removal uses `-ErrorAction SilentlyContinue` so re-running
# on an already-clean tree is a no-op.
#
# Targets (under `<repo-root>/.claude/`):
#
#   - .qa-reports/             (legacy aggregate QA report dir)
#   - .pipeline-states/        (legacy per-spec JSON markers)
#   - .economy-baselines.json  (now per-spec under spec/<name>/economy-baselines.json)
#   - .scan-dispatch.json      (now under .cache/)
#   - .detect-cache.json       (now under .cache/detect.json)
#   - .knowledge-seen.json     (now under .cache/)
#   - .memory-seen.json        (now under .cache/)

$ErrorActionPreference = 'Continue'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
# scripts/ lives at apps/rt/scripts/, repo root is three levels up.
$repoRoot  = (Resolve-Path (Join-Path $scriptDir '..\..\..')).Path
$claudeDir = Join-Path $repoRoot '.claude'

if (-not (Test-Path $claudeDir)) {
    Write-Output ("no .claude/ at " + $repoRoot + " -- nothing to clean")
    exit 0
}

$targets = @(
    '.qa-reports',
    '.pipeline-states',
    '.economy-baselines.json',
    '.scan-dispatch.json',
    '.detect-cache.json',
    '.knowledge-seen.json',
    '.memory-seen.json'
)

$removed = @()
$skipped = @()
foreach ($name in $targets) {
    $path = Join-Path $claudeDir $name
    if (Test-Path $path) {
        Remove-Item -Recurse -Force -Path $path -ErrorAction SilentlyContinue
        if (-not (Test-Path $path)) {
            $removed += $name
        } else {
            $skipped += $name
        }
    } else {
        $skipped += $name
    }
}

$removedCount = $removed.Count
$skippedCount = $skipped.Count
Write-Output ("cleanup-legacy-claude: removed=" + $removedCount + " skipped=" + $skippedCount)
if ($removedCount -gt 0) {
    foreach ($name in $removed) {
        Write-Output ("  removed: " + $name)
    }
}
