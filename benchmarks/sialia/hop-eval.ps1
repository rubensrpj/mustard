# hop-eval.ps1 — measures the LLM SELECTION HOP (claude Haiku picks from the
# deterministic candidate pool) end-to-end through the PRODUCT path:
# `mustard-rt run feature --intent "<pt>"` inside a temp project whose
# mustard.json opts in (`retrieval.hop = "haiku"`). The deterministic funnel
# (gloss + PT+equivalences rank + digest, RRF k=60) is the same committed code;
# the hop is the ONLY delta measured.
#
# Ruler IDENTICAL to the whole series (compare-equiv.ps1 / fused.ps1): scored
# n=13, hit = target OR any secondary in top-K, exact path equality after
# backslash→slash normalization.
#
# STOP RULE (the written bar): hop Acc@5 >= 11/13 -> VENCEU; anything less ->
# MORRE. Reported without makeup, next to the deterministic 46.2%@5 baseline
# re-run in the same session (sanity).
#
# Read-only against C:\Atiz\sialia (never touched — the model artifacts are
# prebuilt copies) and everything outside the temp project.

param(
    [string]$RtExe      = (Join-Path $PSScriptRoot '..\..\target\release\mustard-rt.exe'),
    [string]$ModelSrc   = (Join-Path $PSScriptRoot 'model\grain.model.json'),
    [string]$DictSrc    = (Join-Path $PSScriptRoot 'model\grain.dictionary.json'),
    [string]$EquivSrc   = (Join-Path $PSScriptRoot 'equivalences-mt.json'),
    [string]$LabelsPath = (Join-Path $PSScriptRoot 'labels.ndjson'),
    [string]$OutPath    = (Join-Path $PSScriptRoot 'hop-results.md'),
    [string]$RawPath    = (Join-Path $PSScriptRoot 'hop-raw.json'),
    [string]$ProjDir    = (Join-Path $env:TEMP 'mustard-hop-eval')
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Text.UTF8Encoding]::new($false)

foreach ($p in @($RtExe, $ModelSrc, $DictSrc, $EquivSrc, $LabelsPath)) {
    if (-not (Test-Path -LiteralPath $p)) { throw "missing prerequisite: $p" }
}
# scan.exe must sit beside mustard-rt.exe (Scan::locate contract); the
# translate sidecar is optional (gloss degrades fail-open without it).
$rtDir = Split-Path -Parent (Resolve-Path $RtExe)
if (-not (Test-Path (Join-Path $rtDir 'scan.exe'))) { throw "scan.exe not beside mustard-rt.exe: $rtDir" }
$glossOn = Test-Path (Join-Path $rtDir 'mustard-translate.exe')

# ---- temp project (the ONLY writable surface) -------------------------------------------
Remove-Item $ProjDir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $ProjDir '.claude') | Out-Null
Copy-Item $ModelSrc (Join-Path $ProjDir '.claude\grain.model.json')
Copy-Item $DictSrc  (Join-Path $ProjDir '.claude\grain.dictionary.json')
Copy-Item $EquivSrc (Join-Path $ProjDir '.claude\grain.equivalences.json')
Set-Content (Join-Path $ProjDir 'mustard.json') '{"retrieval":{"hop":"haiku"}}' -Encoding UTF8
Write-Host "temp project: $ProjDir (gloss sidecar: $glossOn)"

# ---- ruler helpers (verbatim shape from fused.ps1 / compare-equiv.ps1) ------------------
function Find-Rank { param($files, [string]$t)
    if ([string]::IsNullOrWhiteSpace($t)) { return -2 }
    $t = $t.Replace('\','/'); for ($i=0;$i -lt $files.Count;$i++){ if ($files[$i] -eq $t){ return ($i+1) } }; return -1 }
function HitK { param([int]$tr,[int[]]$sr,[int]$k)
    if ($tr -ge 1 -and $tr -le $k){ return $true }; foreach($r in @($sr)){ if($r -ge 1 -and $r -le $k){ return $true } }; return $false }
function Best-Rank { param($files, $lab)
    $best = -1
    $trk = Find-Rank $files ([string]$lab.target)
    if ($trk -ge 1) { $best = $trk }
    foreach ($s in @($lab.secondary)) {
        $r = Find-Rank $files $s
        if ($r -ge 1 -and ($best -lt 1 -or $r -lt $best)) { $best = $r }
    }
    return $best
}

# ---- one product run --------------------------------------------------------------------
function Run-Feature { param([string]$pt, [string]$hopMode)
    Push-Location $ProjDir
    $env:MUSTARD_RETRIEVAL_HOP = $hopMode
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $raw = & $RtExe run feature --intent $pt 2>$null | Out-String
    $sw.Stop()
    Pop-Location
    $idx = $raw.IndexOf('{'); if ($idx -lt 0) { throw "no JSON from run feature ($hopMode): $pt" }
    $j = $raw.Substring($idx) | ConvertFrom-Json -Depth 64
    [ordered]@{
        files    = @(@($j.insumos) | ForEach-Object { ([string]$_.file).Replace('\','/') })
        insumos  = @($j.insumos)
        mode     = [string]$j.insumosMode
        hop      = $j.hop
        gloss    = [string]$j.gloss
        wallSecs = [math]::Round($sw.Elapsed.TotalSeconds, 1)
    }
}

# ---- labels ------------------------------------------------------------------------------
$labels = @(); foreach ($line in (Get-Content -LiteralPath $LabelsPath)) { $t=$line.Trim(); if ($t.Length){ $labels += ($t | ConvertFrom-Json -Depth 64) } }
$scored = @($labels | Where-Object { $_.scored })
Write-Host "labels: $($labels.Count) total, $($scored.Count) scored"

# ---- passes: deterministic sanity, then hop ---------------------------------------------
$perId = [ordered]@{}
foreach ($lab in $scored) {
    $id = [int]$lab.id; $pt = [string]$lab.pt
    $det = Run-Feature $pt 'off'
    $hop = Run-Feature $pt 'haiku'
    $perId["$id"] = [ordered]@{ pt = $pt; det = $det; hop = $hop }
    $dr = Best-Rank $det.files $lab; $hr = Best-Rank $hop.files $lab
    Write-Host ("  id{0,-3} det bestRank={1,2}  hop bestRank={2,2} mode={3} calls={4} requeried={5} {6}s" -f `
        $id, $dr, $hr, $hop.mode, $hop.hop.calls, $hop.hop.requeried, $hop.wallSecs)
}

# ---- aggregate ----------------------------------------------------------------------------
function Score-Pass { param([string]$key)
    $h5=0; $h10=0; $ids=@()
    foreach ($lab in $scored) {
        $files = $perId[[string]$lab.id][$key].files
        $trk = Find-Rank $files ([string]$lab.target)
        $sr = @(); foreach($s in @($lab.secondary)){ $sr += (Find-Rank $files $s) }
        if (HitK $trk $sr 5)  { $h5++; $ids += [int]$lab.id }
        if (HitK $trk $sr 10) { $h10++ }
    }
    @{ h5=$h5; h10=$h10; n=$scored.Count; ids=(@($ids) | Sort-Object) }
}
$detAgg = Score-Pass 'det'
$hopAgg = Score-Pass 'hop'
$hopRuns   = @($perId.Keys | ForEach-Object { $perId[$_].hop })
$hopCalls  = ($hopRuns | ForEach-Object { [int]$_.hop.calls } | Measure-Object -Sum).Sum
$requeries = @($hopRuns | Where-Object { $_.hop.requeried }).Count
$fellBack  = @($hopRuns | Where-Object { $_.mode -ne 'hop' }).Count
$avgWall   = [math]::Round((($hopRuns | ForEach-Object { [double]$_.wallSecs } | Measure-Object -Average).Average), 1)
$avgHopMs  = [math]::Round((($hopRuns | ForEach-Object { [double]$_.hop.durationMs } | Measure-Object -Average).Average), 0)
$avgInTok  = [math]::Round((($hopRuns | ForEach-Object { [double]$_.hop.inputTokens } | Measure-Object -Average).Average), 0)
$avgOutTok = [math]::Round((($hopRuns | ForEach-Object { [double]$_.hop.outputTokens } | Measure-Object -Average).Average), 0)
$totInTok  = ($hopRuns | ForEach-Object { [long]$_.hop.inputTokens } | Measure-Object -Sum).Sum
$totOutTok = ($hopRuns | ForEach-Object { [long]$_.hop.outputTokens } | Measure-Object -Sum).Sum

$pct5  = [math]::Round(100.0*$hopAgg.h5/$hopAgg.n, 1)
$pct10 = [math]::Round(100.0*$hopAgg.h10/$hopAgg.n, 1)
$won = ($hopAgg.h5 -ge 11)
$verdict = if ($won) { 'VENCEU' } else { 'MORRE' }

# ---- id15 stress: the 4 partner-portal sections through the hop --------------------------
$lab15 = $labels | Where-Object { [int]$_.id -eq 15 } | Select-Object -First 1
$sections = @(([string]$lab15.pt) -split ';' | ForEach-Object { $_.Trim() } | Where-Object { $_.Length })
if ($sections.Count -ne 4) { throw "expected 4 id15 sections, got $($sections.Count)" }
$stress = @()
foreach ($sec in $sections) {
    $r = Run-Feature $sec 'haiku'
    $stress += [ordered]@{ pt = $sec; mode = $r.mode; wallSecs = $r.wallSecs; top5 = @($r.insumos | Select-Object -First 5) }
    Write-Host ("  stress ({0}): {1}..." -f $r.mode, $sec.Substring(0, [math]::Min(60, $sec.Length)))
}

# ---- report -------------------------------------------------------------------------------
$sb = [Text.StringBuilder]::new()
$null=$sb.AppendLine('# Salto de selecao (LLM hop, Haiku) sobre o funil deterministico — medicao final')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("**Hop Acc@5: $($hopAgg.h5)/$($hopAgg.n) ($pct5%) vs barra >=11/13 (84.6%) -> veredito: $verdict.** Deterministico na mesma sessao: $($detAgg.h5)/$($detAgg.n) @5 (referencia historica 46.2%).")
$null=$sb.AppendLine('')
$null=$sb.AppendLine("Generated $(Get-Date -Format 'yyyy-MM-dd HH:mm') by ``hop-eval.ps1``. Product path: ``mustard-rt run feature --intent <pt>`` em projeto temporario com ``retrieval.hop=haiku`` (modelo sialia pre-buildado; equivalences = artefato C2 ``equivalences-mt.json``; gloss sidecar: $glossOn). Pool ~25 candidatos (RRF k=60 rank+digest com evidencia por linha), 1 chamada ``claude -p --model claude-haiku-4-5-20251001`` (cwd neutro sem .claude, timeout 45s, fail-open), re-query no maximo 1 quando o modelo pede e menos de 5 picks validos. Ruler identico a serie: n=$($hopAgg.n), hit = target OU secundario, igualdade exata de caminho.")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## Acc@5 / Acc@10 (n=13)')
$null=$sb.AppendLine('')
$null=$sb.AppendLine('| Variant | Acc@5 | Acc@10 | hit ids @5 |')
$null=$sb.AppendLine('|---|---|---|---|')
$dp5 = [math]::Round(100.0*$detAgg.h5/$detAgg.n,1); $dp10 = [math]::Round(100.0*$detAgg.h10/$detAgg.n,1)
$null=$sb.AppendLine("| deterministico (sanidade, mesma sessao) | $($detAgg.h5)/$($detAgg.n) ($dp5%) | $($detAgg.h10)/$($detAgg.n) ($dp10%) | $(@($detAgg.ids) -join ',') |")
$null=$sb.AppendLine("| **hop haiku (produto)** | **$($hopAgg.h5)/$($hopAgg.n) ($pct5%)** | **$($hopAgg.h10)/$($hopAgg.n) ($pct10%)** | $(@($hopAgg.ids) -join ',') |")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## Custo / latencia do hop (13 queries)')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("- chamadas claude: $hopCalls (re-queries disparadas: $requeries; fallbacks para deterministico: $fellBack)")
$null=$sb.AppendLine("- latencia media: hop $($avgHopMs)ms por query; run completa (funil+hop) $($avgWall)s de parede")
$null=$sb.AppendLine("- tokens medios por query: in $avgInTok (inclui cache do CLI) / out $avgOutTok; total da medicao: in $totInTok / out $totOutTok")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## Per-label best rank (target-or-secondary)')
$null=$sb.AppendLine('')
$null=$sb.AppendLine('| id | det | hop | hit@5 det | hit@5 hop | mode | requeried |')
$null=$sb.AppendLine('|---|---|---|---|---|---|---|')
foreach ($lab in $scored) {
    $id = [int]$lab.id; $p = $perId["$id"]
    $rd = Best-Rank $p.det.files $lab; $rh = Best-Rank $p.hop.files $lab
    $hd = if ($rd -ge 1 -and $rd -le 5) { 'Y' } else { '.' }
    $hh = if ($rh -ge 1 -and $rh -le 5) { 'Y' } else { '.' }
    $null=$sb.AppendLine("| $id | $rd | $rh | $hd | $hh | $($p.hop.mode) | $($p.hop.hop.requeried) |")
}
$null=$sb.AppendLine('')
$null=$sb.AppendLine('(-1 = fora do top-10 emitido; ranks sao o MELHOR entre alvo e secundarios.)')
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## REGRA DE PARADA')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("**Veredito: $verdict** — regra escrita antes da medicao: hop Acc@5 >= 11/13 -> VENCEU; senao MORRE. Resultado: $($hopAgg.h5)/13 ($pct5%) @5, $($hopAgg.h10)/13 ($pct10%) @10.")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## Caso-estresse id15 — as 4 secoes do prompt sialia-partners pelo hop')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("Referencias do gabarito id15: target ``$($lab15.target)``; secundarios $(@($lab15.secondary | ForEach-Object { '``'+$_+'``' }) -join ', ').")
foreach ($s in $stress) {
    $null=$sb.AppendLine('')
    $null=$sb.AppendLine("### $($s.pt)")
    $null=$sb.AppendLine('')
    $null=$sb.AppendLine("(mode=$($s.mode), $($s.wallSecs)s)")
    $null=$sb.AppendLine('')
    $i=0; foreach ($row in $s.top5) { $i++; $why = if ($row.why) { " — $($row.why)" } else { '' }; $null=$sb.AppendLine("$i. ``$($row.file)`` [$($row.source)]$why") }
    $null=$sb.AppendLine('')
    $null=$sb.AppendLine('Avaliacao: _(preenchida a mao apos a run)_')
}
Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8

[ordered]@{ perId = $perId; det = $detAgg; hop = $hopAgg; stress = $stress } |
    ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $RawPath -Encoding UTF8

Write-Host ''
Write-Host ("=== HOP: {0}/{1} ({2}%) @5 vs barra 11/13 — {3} ===" -f $hopAgg.h5, $hopAgg.n, $pct5, $verdict)
Write-Host "Wrote $OutPath"
Write-Host "Wrote $RawPath"
