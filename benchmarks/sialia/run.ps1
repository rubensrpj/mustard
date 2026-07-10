# run.ps1 - Baseline harness for the mustard retrieval feature (the "digest").
#
# Measures `mustard-rt run feature --intent "<txt>"` (executed with cwd = sialia)
# against the labeled prompt set in labels.ndjson, in two modes:
#   (a) CRU   : --intent "<pt>"                (raw Portuguese)
#   (b) JUSTO : --intent "<pt> -- <en>"        (fair: PT plus its EN gloss)
#
# Ranked file list for one prompt = every {file, scoreX1024} from the top-level
# `anchorsDetail[]` UNION every `concerns[].anchorsDetail`, keeping the MAX score
# per distinct file, sorted score DESC (path ASC as a deterministic tiebreak).
#
# Read-only against sialia. Writes only inside this folder. No Python.

param(
    [string]$Sialia     = 'C:\Atiz\sialia',
    [string]$LabelsPath = (Join-Path $PSScriptRoot 'labels.ndjson'),
    [string]$OutPath    = (Join-Path $PSScriptRoot 'baseline-results.md'),
    [string]$RawPath    = (Join-Path $PSScriptRoot 'baseline-raw.json')
)

$ErrorActionPreference = 'Stop'

# ---- helpers ---------------------------------------------------------------

function Get-RankedFiles {
    # Runs the digest for one intent, returns:
    #   @{ files=[ @{file;score} ... sorted ]; error; withheld; miss; concernCount; anchorCount }
    param([string]$Intent)

    if ([string]::IsNullOrWhiteSpace($Intent)) {
        return @{ files=@(); error='empty-intent'; withheld=$false; miss=$false; concernCount=0; anchorCount=0 }
    }

    Set-Location -LiteralPath $Sialia
    $raw = & mustard-rt run feature --intent $Intent 2>$null
    $text = ($raw | Out-String)

    $idx = $text.IndexOf('{')
    if ($idx -lt 0) {
        return @{ files=@(); error='no-json'; withheld=$false; miss=$false; concernCount=0; anchorCount=0 }
    }

    try {
        $obj = $text.Substring($idx) | ConvertFrom-Json -Depth 64
    } catch {
        return @{ files=@(); error='parse-fail'; withheld=$false; miss=$false; concernCount=0; anchorCount=0 }
    }

    $map = @{}   # normalized file path -> max scoreX1024

    # Gather every anchorsDetail entry: top-level, then each concern.
    $details = [System.Collections.ArrayList]::new()
    foreach ($a in @($obj.anchorsDetail)) { if ($null -ne $a) { [void]$details.Add($a) } }
    $concernCount = 0
    foreach ($c in @($obj.concerns)) {
        if ($null -eq $c) { continue }
        $concernCount++
        foreach ($a in @($c.anchorsDetail)) { if ($null -ne $a) { [void]$details.Add($a) } }
    }

    foreach ($a in $details) {
        if ($null -eq $a.file) { continue }
        $f = ([string]$a.file).Replace('\','/')
        $s = [int]$a.scoreX1024
        if (-not $map.ContainsKey($f) -or $map[$f] -lt $s) { $map[$f] = $s }
    }

    $ranked = @(
        $map.GetEnumerator() |
            ForEach-Object { [pscustomobject]@{ file = $_.Key; score = [int]$_.Value } } |
            Sort-Object -Property @{Expression='score';Descending=$true}, @{Expression='file';Descending=$false}
    )

    return @{
        files        = $ranked
        error        = $null
        withheld     = [bool]$obj.planningWithheld
        miss         = [bool]$obj.miss
        concernCount = $concernCount
        anchorCount  = $ranked.Count
    }
}

function Find-Rank {
    # -2 = no target defined (n/a); -1 = target defined but not in list; else 1-based rank
    param($rankedFiles, [string]$target)
    if ([string]::IsNullOrWhiteSpace($target)) { return -2 }
    $t = $target.Replace('\','/')
    for ($i = 0; $i -lt $rankedFiles.Count; $i++) {
        if ($rankedFiles[$i].file -eq $t) { return ($i + 1) }
    }
    return -1
}

function Fmt-Rank {
    param([int]$r)
    switch ($r) {
        -2 { 'n/a' }
        -1 { 'miss' }
        default { "$r" }
    }
}

function Test-HitAtK {
    # target OR any secondary within top-K
    param([int]$TargetRank, [int[]]$SecRanks, [int]$K)
    if ($TargetRank -ge 1 -and $TargetRank -le $K) { return $true }
    foreach ($r in @($SecRanks)) { if ($r -ge 1 -and $r -le $K) { return $true } }
    return $false
}

function YN([bool]$b) { if ($b) { 'Y' } else { '.' } }

# ---- load labels -----------------------------------------------------------

$labels = @()
foreach ($line in (Get-Content -LiteralPath $LabelsPath)) {
    $t = $line.Trim()
    if ($t.Length -eq 0) { continue }
    $labels += ($t | ConvertFrom-Json -Depth 64)
}
Write-Host "Loaded $($labels.Count) labels. Running $($labels.Count * 2) digest invocations..."

# ---- run -------------------------------------------------------------------

$results = @()
foreach ($lab in $labels) {
    Write-Host ("  id {0,-2} [{1}] ..." -f $lab.id, $lab.difficulty)

    $cru       = Get-RankedFiles -Intent ([string]$lab.pt)
    $justoText = "$($lab.pt) -- $($lab.en)"
    $justo     = Get-RankedFiles -Intent $justoText

    $sec = @($lab.secondary)

    $tRankCru   = Find-Rank $cru.files   $lab.target
    $tRankJusto = Find-Rank $justo.files $lab.target

    $secRanksCru   = @(); foreach ($s in $sec) { $secRanksCru   += (Find-Rank $cru.files   $s) }
    $secRanksJusto = @(); foreach ($s in $sec) { $secRanksJusto += (Find-Rank $justo.files $s) }

    $results += [pscustomobject]@{
        id            = [int]$lab.id
        difficulty    = [string]$lab.difficulty
        ambiguous     = [bool]$lab.ambiguous
        scored        = [bool]$lab.scored
        target        = [string]$lab.target
        secondary     = $sec
        note          = [string]$lab.note
        # cru
        cruErr        = $cru.error
        cruWithheld   = $cru.withheld
        cruMiss       = $cru.miss
        cruTargetRank = $tRankCru
        cruSecRanks   = $secRanksCru
        cruHit5       = (Test-HitAtK $tRankCru $secRanksCru 5)
        cruHit10      = (Test-HitAtK $tRankCru $secRanksCru 10)
        cruTop        = @($cru.files | Select-Object -First 6)
        cruCount      = $cru.anchorCount
        # justo
        justoErr        = $justo.error
        justoWithheld   = $justo.withheld
        justoMiss       = $justo.miss
        justoTargetRank = $tRankJusto
        justoSecRanks   = $secRanksJusto
        justoHit5       = (Test-HitAtK $tRankJusto $secRanksJusto 5)
        justoHit10      = (Test-HitAtK $tRankJusto $secRanksJusto 10)
        justoTop        = @($justo.files | Select-Object -First 6)
        justoCount      = $justo.anchorCount
    }
}

# ---- aggregate (scored:true only) ------------------------------------------

$scored = @($results | Where-Object { $_.scored })
$n = $scored.Count

$cruHit5   = @($scored | Where-Object { $_.cruHit5 }).Count
$cruHit10  = @($scored | Where-Object { $_.cruHit10 }).Count
$justoHit5 = @($scored | Where-Object { $_.justoHit5 }).Count
$justoHit10= @($scored | Where-Object { $_.justoHit10 }).Count

function Pct([int]$k, [int]$tot) { if ($tot -eq 0) { '0.0' } else { ('{0:N1}' -f (100.0 * $k / $tot)) } }

# ---- render markdown -------------------------------------------------------

$sb = [System.Text.StringBuilder]::new()
$null = $sb.AppendLine("# Baseline - retrieval (digest) vs sialia labels")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("Generated by ``benchmarks/sialia/run.ps1`` on $(Get-Date -Format 'yyyy-MM-dd HH:mm').")
$null = $sb.AppendLine("Retrieval under test: ``mustard-rt run feature --intent`` executed with cwd = ``$Sialia`` (read-only).")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("- **CRU** = raw Portuguese intent (``<pt>``).")
$null = $sb.AppendLine("- **JUSTO** = Portuguese plus English gloss (``<pt> -- <en>``).")
$null = $sb.AppendLine("- Ranked list = union of top-level ``anchorsDetail`` and every ``concerns[].anchorsDetail``, max ``scoreX1024`` per distinct file, sorted score DESC (path ASC tiebreak).")
$null = $sb.AppendLine("- ``rank`` = 1-based position of the **target**; ``miss`` = target defined but absent; ``n/a`` = no target (id 10).")
$null = $sb.AppendLine("- ``hit@5`` = target OR any secondary within top-5.")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("## Aggregate (scored labels only, n=$n)")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("| Metric | CRU (PT) | JUSTO (PT+EN) |")
$null = $sb.AppendLine("|---|---|---|")
$null = $sb.AppendLine("| Acc@5  | $cruHit5/$n ($(Pct $cruHit5 $n)%) | $justoHit5/$n ($(Pct $justoHit5 $n)%) |")
$null = $sb.AppendLine("| Acc@10 | $cruHit10/$n ($(Pct $cruHit10 $n)%) | $justoHit10/$n ($(Pct $justoHit10 $n)%) |")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("## Per-prompt")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("| id | diff | scored | rank(target) CRU | rank(target) JUSTO | hit@5 CRU | hit@5 JUSTO | hit@10 CRU | hit@10 JUSTO |")
$null = $sb.AppendLine("|---:|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|")
foreach ($r in $results) {
    $sc = if ($r.scored) { 'yes' } else { 'no' }
    $null = $sb.AppendLine(("| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7} | {8} |" -f `
        $r.id, $r.difficulty, $sc, (Fmt-Rank $r.cruTargetRank), (Fmt-Rank $r.justoTargetRank), `
        (YN $r.cruHit5), (YN $r.justoHit5), (YN $r.cruHit10), (YN $r.justoHit10)))
}
$null = $sb.AppendLine("")
$null = $sb.AppendLine("Note: id 10 & id 15 are ``scored:false`` (excluded from the aggregate).")

# --- crossling movers: scored labels that missed in CRU but hit in JUSTO ---
$movers = @($scored | Where-Object { -not $_.cruHit5 -and $_.justoHit5 })
$regress = @($scored | Where-Object { $_.cruHit5 -and -not $_.justoHit5 })
$null = $sb.AppendLine("")
$null = $sb.AppendLine("## Cross-lingual movers (hit@5)")
$null = $sb.AppendLine("")
if ($movers.Count -gt 0) {
    $null = $sb.AppendLine("Missed in CRU, recovered in JUSTO: " + (($movers | ForEach-Object { "id $($_.id)" }) -join ', '))
} else {
    $null = $sb.AppendLine("Missed in CRU, recovered in JUSTO: (none)")
}
if ($regress.Count -gt 0) {
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("Regressed (hit in CRU, missed in JUSTO): " + (($regress | ForEach-Object { "id $($_.id)" }) -join ', '))
}

# --- id 10 anti-hallucination ----------------------------------------------
$r10 = $results | Where-Object { $_.id -eq 10 } | Select-Object -First 1
if ($r10) {
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("## id 10 - anti-hallucination (no valid target)")
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("A good retrieval should NOT point confidently anywhere (``CommissionType`` does not exist in the model).")
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("- CRU:   withheld=$($r10.cruWithheld) miss=$($r10.cruMiss) anchors=$($r10.cruCount)")
    $null = $sb.AppendLine("- JUSTO: withheld=$($r10.justoWithheld) miss=$($r10.justoMiss) anchors=$($r10.justoCount)")
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("Top-3 CRU:")
    if (@($r10.cruTop).Count -eq 0) { $null = $sb.AppendLine("- (empty)") }
    foreach ($f in @($r10.cruTop | Select-Object -First 3)) { $null = $sb.AppendLine("- $($f.score)  ``$($f.file)``") }
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("Top-3 JUSTO:")
    if (@($r10.justoTop).Count -eq 0) { $null = $sb.AppendLine("- (empty)") }
    foreach ($f in @($r10.justoTop | Select-Object -First 3)) { $null = $sb.AppendLine("- $($f.score)  ``$($f.file)``") }
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("(Expected-adjacent secondary, if the engine surfaces it: ``$($r10.secondary -join ', ')``)")
}

# --- id 15 multi-concern stress --------------------------------------------
$r15 = $results | Where-Object { $_.id -eq 15 } | Select-Object -First 1
if ($r15) {
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("## id 15 - multi-concern stress case (top-5 JUSTO)")
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("Primary target: ``$($r15.target)`` -> rank JUSTO = $(Fmt-Rank $r15.justoTargetRank), rank CRU = $(Fmt-Rank $r15.cruTargetRank).")
    $null = $sb.AppendLine("Secondary ranks JUSTO: " + (($r15.secondary | ForEach-Object { $i = [array]::IndexOf($r15.secondary, $_); "``$_`` = $(Fmt-Rank $r15.justoSecRanks[$i])" }) -join '; '))
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("Top-5 JUSTO:")
    if (@($r15.justoTop).Count -eq 0) { $null = $sb.AppendLine("- (empty / withheld=$($r15.justoWithheld))") }
    foreach ($f in @($r15.justoTop | Select-Object -First 5)) { $null = $sb.AppendLine("- $($f.score)  ``$($f.file)``") }
}

# --- errors / withheld ledger ----------------------------------------------
$anyErr = @($results | Where-Object { $_.cruErr -or $_.justoErr -or $_.cruWithheld -or $_.justoWithheld })
$null = $sb.AppendLine("")
$null = $sb.AppendLine("## Honesty ledger (errors / withheld / empty)")
$null = $sb.AppendLine("")
if ($anyErr.Count -eq 0) {
    $null = $sb.AppendLine("No errors, no withheld planning, no empty lists.")
} else {
    foreach ($r in $anyErr) {
        $null = $sb.AppendLine("- id $($r.id): cruErr=$($r.cruErr) cruWithheld=$($r.cruWithheld) cruCount=$($r.cruCount) | justoErr=$($r.justoErr) justoWithheld=$($r.justoWithheld) justoCount=$($r.justoCount)")
    }
}

Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8

# raw json sidecar for determinism auditing
$results | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $RawPath -Encoding UTF8

# ---- console summary -------------------------------------------------------
Write-Host ""
Write-Host "=== AGGREGATE (scored n=$n) ==="
Write-Host ("Acc@5   CRU {0}/{1} ({2}%)   JUSTO {3}/{1} ({4}%)" -f $cruHit5, $n, (Pct $cruHit5 $n), $justoHit5, (Pct $justoHit5 $n))
Write-Host ("Acc@10  CRU {0}/{1} ({2}%)   JUSTO {3}/{1} ({4}%)" -f $cruHit10, $n, (Pct $cruHit10 $n), $justoHit10, (Pct $justoHit10 $n))
Write-Host ("movers CRU-miss->JUSTO-hit @5: " + (($movers | ForEach-Object { $_.id }) -join ','))
Write-Host "Wrote $OutPath"
Write-Host "Wrote $RawPath"
