#!/usr/bin/env pwsh
# W4 of 2026-05-25-mustard-deep-refactor — emit pipeline.status for every
# archived spec under ~/.mustard-backups/2026-05-25-specs-archive so the
# telemetry.db reflects the final outcome.
#
# Mapping (per the wave spec):
#   2026-05-24-mustard-unification              -> Completed
#   2026-05-21-mustard-v1-installer-and-update  -> Cancelled
#   2026-05-20-dashboard-prd-ai-lapidator       -> Cancelled
#   *-SUPERSEDED                                -> Superseded
#   2026-05-24-config-idioma-tom                -> Absorbed
#   2026-05-24-meta-sidecar                     -> Absorbed
#   2026-05-23-per-spec-event-log-*             -> Absorbed
#   2026-05-23-tf-dashboard-page-primitives     -> Completed
#   2026-05-23-tf-dashboard-ds-tokens-remap     -> Completed
#   2026-05-23-tf-dashboard-eslint-baseline     -> Completed
#   everything else                             -> Completed

$ErrorActionPreference = "Stop"

$ArchiveRoot = Join-Path $env:USERPROFILE ".mustard-backups\2026-05-25-specs-archive"
if (-not (Test-Path $ArchiveRoot)) {
    Write-Error "Archive root not found: $ArchiveRoot"
    exit 1
}

# Top-level spec dirs only — wave subdirs are children of a spec and inherit
# the parent's outcome (no event emitted for them).
$specs = Get-ChildItem -Path $ArchiveRoot -Directory | Select-Object -ExpandProperty Name

function Resolve-Outcome($name) {
    if ($name -eq "2026-05-24-mustard-unification") { return "Completed" }
    if ($name -eq "2026-05-21-mustard-v1-installer-and-update") { return "Cancelled" }
    if ($name -eq "2026-05-20-dashboard-prd-ai-lapidator") { return "Cancelled" }
    if ($name -like "*-SUPERSEDED") { return "Superseded" }
    if ($name -eq "2026-05-24-config-idioma-tom") { return "Absorbed" }
    if ($name -eq "2026-05-24-meta-sidecar") { return "Absorbed" }
    if ($name -like "2026-05-23-per-spec-event-log-*") { return "Absorbed" }
    if ($name -eq "2026-05-23-tf-dashboard-page-primitives") { return "Completed" }
    if ($name -eq "2026-05-23-tf-dashboard-ds-tokens-remap") { return "Completed" }
    if ($name -eq "2026-05-23-tf-dashboard-eslint-baseline") { return "Completed" }
    return "Completed"
}

$total = $specs.Count
$ok = 0
$fail = 0
$counts = @{ Completed = 0; Cancelled = 0; Superseded = 0; Absorbed = 0 }

foreach ($spec in $specs) {
    $outcome = Resolve-Outcome $spec
    $valueLower = $outcome.ToLower()
    # `to` is the canonical PipelineStatusPayload field name (see
    # mustard-core::model::event::PipelineStatusPayload). `reason` is a free
    # extra carried for audit/archive context — readers ignore unknown fields.
    $payload = (@{ to = $valueLower; reason = "archived in deep-refactor consolidation" } | ConvertTo-Json -Compress)
    # PowerShell + emit-pipeline: payload must arrive as a single JSON string arg.
    & mustard-rt run emit-pipeline --kind pipeline.status --spec $spec --payload $payload 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) {
        $ok++
        $counts[$outcome]++
    } else {
        $fail++
        Write-Host "FAILED: $spec (exit $LASTEXITCODE)"
    }
}

Write-Host ""
Write-Host "Emitted $ok / $total events ($fail failures)"
Write-Host "Outcome breakdown:"
foreach ($k in $counts.Keys) {
    Write-Host ("  {0,-12} {1}" -f $k, $counts[$k])
}
