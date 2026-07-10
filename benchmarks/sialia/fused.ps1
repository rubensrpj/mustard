# fused.ps1 — FINAL retrieval experiment: lexical grain-rank × local dense embeddings,
# fused with Reciprocal Rank Fusion (RRF). The lexical+graph paradigm plateaus at
# 6/13 (46.2%) Acc@5 / 7/13 (53.8%) @10 on this label set (confirmed 3×); the dense
# side is the ONE remaining lever with literature evidence. Everything below is
# zero-Claude at query time: grain rank (deterministic), mustard-translate (local
# Marian MT), mustard-embed (local multilingual-e5-SMALL via ONNX — the small
# model is what fits the ≤15-min full-repo index-build budget).
#
#   list A : scan.exe rank — query = raw PT + equivalences-mt tokens (the current
#            winner, C2 shape), top $Depth files.
#   list B : mustard-embed search — TWO query variants, measured separately:
#            B_pt = --intent "<raw pt>"   (multilingual model eats PT directly)
#            B_en = --intent "<MT-EN>"    (pt → mustard-translate → EN)
#   fusion : RRF  score(f) = Σ_lists 1/(k + rank_f)   with k ∈ {60, 20}.
#
# Ruler IDENTICAL to compare-equiv.ps1: scored n=13, hit = target OR any secondary
# in top-K, exact path equality after backslash→slash normalization.
#
# STOP RULE (written into the report): best fused Acc@5 ≥ 56% (≥ 8/13) AND full
# index build ≤ 15 min → the fusion WINS and becomes the final architecture;
# EITHER failing → STOP — the 46/54 short-list + agent hop is the final form.
# Build time is a first-class product acceptance criterion, independent of
# accuracy. No tuning beyond the 2×2 grid above.
#
# Read-only against C:\Atiz\sialia and everything outside this folder.

param(
    [string]$Exe          = (Join-Path $PSScriptRoot '..\..\target\release\scan.exe'),
    [string]$EmbedExe     = (Join-Path $PSScriptRoot '..\..\apps\embed\target\release\mustard-embed.exe'),
    [string]$TranslateExe = (Join-Path $PSScriptRoot '..\..\apps\translate\target\release\mustard-translate.exe'),
    [string]$Model        = (Join-Path $PSScriptRoot 'model\grain.model.json'),
    [string]$Dict         = (Join-Path $PSScriptRoot 'model\grain.dictionary.json'),
    [string]$Vectors      = (Join-Path $PSScriptRoot 'model\grain.vectors'),
    [string]$EquivPath    = (Join-Path $PSScriptRoot 'equivalences-mt.json'),
    [string]$LabelsPath   = (Join-Path $PSScriptRoot 'labels.ndjson'),
    [string]$OutPath      = (Join-Path $PSScriptRoot 'fused-results.md'),
    [string]$RawPath      = (Join-Path $PSScriptRoot 'fused-raw.json'),
    [string]$Base         = '100000',
    [int]$Depth           = 20,     # list depth fed to RRF from each side (fixed, not tuned)
    [int]$EmbedCandidates = 100,    # method-candidates pool so ≥ $Depth DISTINCT files surface
    # REAL wall time of the full `mustard-embed build` on this corpus, in minutes —
    # measured outside this script; enters the stop rule as a hard criterion (≤ 15).
    [Parameter(Mandatory=$true)][double]$BuildMinutes
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Text.UTF8Encoding]::new($false)

foreach ($p in @($Exe, $EmbedExe, $TranslateExe, $Model, $Dict, $Vectors, $EquivPath, $LabelsPath)) {
    if (-not (Test-Path -LiteralPath $p)) { throw "missing prerequisite: $p" }
}

# ---- ruler helpers (verbatim shape from compare-equiv.ps1) ----------------------------
function Fold-Tok { param([string]$s)
    if ([string]::IsNullOrEmpty($s)) { return '' }
    $n = ($s.ToLowerInvariant()).Normalize([Text.NormalizationForm]::FormD)
    $sb = [Text.StringBuilder]::new()
    foreach ($c in $n.ToCharArray()) { if ([Globalization.CharUnicodeInfo]::GetUnicodeCategory($c) -ne [Globalization.UnicodeCategory]::NonSpacingMark) { $null = $sb.Append($c) } }
    return $sb.ToString().Normalize([Text.NormalizationForm]::FormC)
}
function Load-Equiv { param([string]$p)
    $raw = (Get-Content -Raw -LiteralPath $p | ConvertFrom-Json -Depth 64).equivalences
    $h = @{}; foreach ($prop in $raw.PSObject.Properties) { $h[$prop.Name] = @($prop.Value) }
    return $h
}
function Added-Tokens { param($equiv, [string]$pt)
    $added = [Collections.Generic.List[string]]::new(); $seen = @{}
    foreach ($tok in ($pt -split '[^\p{L}\p{Nd}]+')) {
        if ($tok.Length -lt 3) { continue }
        $f = Fold-Tok $tok
        if ($equiv.ContainsKey($f)) { foreach ($en in $equiv[$f]) { if (-not $seen.ContainsKey($en)) { $seen[$en]=$true; $added.Add($en) } } }
    }
    return $added
}
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

# ---- engines --------------------------------------------------------------------------
function Run-Ranker { param([string]$q)
    if ([string]::IsNullOrWhiteSpace($q)) { return @() }
    $raw = & $Exe rank $Model --dict $Dict --query $q --top $Depth --direct-base $Base 2>$null | Out-String
    $idx = $raw.IndexOf('{'); if ($idx -lt 0) { return @() }
    try { $o = $raw.Substring($idx) | ConvertFrom-Json -Depth 64 } catch { return @() }
    return @($o.files | ForEach-Object { ([string]$_.file).Replace('\','/') })
}
function Run-Embed { param([string]$q)
    if ([string]::IsNullOrWhiteSpace($q)) { return @() }
    # --no-daemon: hermetic per-call cold search (no stale daemon state in a benchmark)
    $raw = & $EmbedExe search --intent $q --vectors $Vectors --top $Depth --candidates $EmbedCandidates --no-daemon 2>$null | Out-String
    $idx = $raw.IndexOf('{'); if ($idx -lt 0) { return @() }
    try { $o = $raw.Substring($idx) | ConvertFrom-Json -Depth 64 } catch { return @() }
    return @($o.files | ForEach-Object { ([string]$_.file).Replace('\','/') })
}
function Fuse-RRF { param([string[]]$listA, [string[]]$listB, [int]$k)
    $score = @{}
    for ($i=0; $i -lt $listA.Count; $i++) { $f=$listA[$i]; $score[$f] = [double]$score[$f] + 1.0/($k + $i + 1) }
    for ($i=0; $i -lt $listB.Count; $i++) { $f=$listB[$i]; $score[$f] = [double]$score[$f] + 1.0/($k + $i + 1) }
    return @($score.GetEnumerator() |
        Sort-Object -Property @{Expression='Value';Descending=$true}, @{Expression='Key';Descending=$false} |
        ForEach-Object { [string]$_.Key })
}

# ---- load labels + equivalences --------------------------------------------------------
$labels = @(); foreach ($line in (Get-Content -LiteralPath $LabelsPath)) { $t=$line.Trim(); if ($t.Length){ $labels += ($t | ConvertFrom-Json -Depth 64) } }
$scored = @($labels | Where-Object { $_.scored })
$equiv  = Load-Equiv $EquivPath
Write-Host "labels: $($labels.Count) total, $($scored.Count) scored; mt-equivalence keys: $($equiv.Count)"

# ---- stress sections: the 4 concerns of id15, literal ';'-split of its pt --------------
$lab15 = $labels | Where-Object { [int]$_.id -eq 15 } | Select-Object -First 1
$sections = @(([string]$lab15.pt) -split ';' | ForEach-Object { $_.Trim() } | Where-Object { $_.Length })
if ($sections.Count -ne 4) { throw "expected 4 id15 sections, got $($sections.Count)" }

# ---- translate all PT texts in ONE mustard-translate batch (1 line in : 1 JSON out) ----
$ptTexts = @($scored | ForEach-Object { [string]$_.pt }) + $sections
Write-Host "translating $($ptTexts.Count) texts via mustard-translate batch..."
$swMt = [Diagnostics.Stopwatch]::StartNew()
$mtRaw = ($ptTexts -join "`n") | & $TranslateExe batch 2>$null
$swMt.Stop()
$mtLines = @($mtRaw | Where-Object { $_ -match '^\{' })
if ($mtLines.Count -ne $ptTexts.Count) { throw "mt batch contract broken: $($mtLines.Count)/$($ptTexts.Count)" }
$mtOf = @{}
for ($i = 0; $i -lt $ptTexts.Count; $i++) { $mtOf[$ptTexts[$i]] = [string](($mtLines[$i] | ConvertFrom-Json -Depth 8).en) }
Write-Host ("  done in {0}s" -f [math]::Round($swMt.Elapsed.TotalSeconds,1))

# ---- per-label lists --------------------------------------------------------------------
$perId = [ordered]@{}
$swAll = [Diagnostics.Stopwatch]::StartNew()
foreach ($lab in $scored) {
    $id = [int]$lab.id
    $pt = [string]$lab.pt
    $added = Added-Tokens $equiv $pt
    $qLex  = ($pt + ' ' + ($added -join ' ')).Trim()
    $en    = $mtOf[$pt]
    $lex   = Run-Ranker $qLex
    $embPt = Run-Embed $pt
    $embEn = Run-Embed $en
    $perId[$id] = [ordered]@{
        pt = $pt; en = $en; qLex = $qLex
        lex = $lex; embPt = $embPt; embEn = $embEn
        fusedPt60 = (Fuse-RRF $lex $embPt 60); fusedPt20 = (Fuse-RRF $lex $embPt 20)
        fusedEn60 = (Fuse-RRF $lex $embEn 60); fusedEn20 = (Fuse-RRF $lex $embEn 20)
    }
    Write-Host ("  id{0,-3} lists ready (lex {1}, embPt {2}, embEn {3})" -f $id, $lex.Count, $embPt.Count, $embEn.Count)
}
$swAll.Stop()

# ---- aggregate --------------------------------------------------------------------------
$variants = [ordered]@{
    'lexical alone (sanity: the 46.2/53.8 bar)' = 'lex'
    'embed alone - PT query'                    = 'embPt'
    'embed alone - MT-EN query'                 = 'embEn'
    'fused RRF k=60 - lex + embed-PT'           = 'fusedPt60'
    'fused RRF k=20 - lex + embed-PT'           = 'fusedPt20'
    'fused RRF k=60 - lex + embed-EN'           = 'fusedEn60'
    'fused RRF k=20 - lex + embed-EN'           = 'fusedEn20'
}
$agg = [ordered]@{}
foreach ($name in $variants.Keys) {
    $key = $variants[$name]; $h5=0; $h10=0; $ids=@()
    foreach ($lab in $scored) {
        $files = $perId[[int]$lab.id][$key]
        $trk = Find-Rank $files ([string]$lab.target)
        $sr = @(); foreach($s in @($lab.secondary)){ $sr += (Find-Rank $files $s) }
        if (HitK $trk $sr 5)  { $h5++; $ids += [int]$lab.id }
        if (HitK $trk $sr 10) { $h10++ }
    }
    $agg[$name] = @{ key=$key; h5=$h5; h10=$h10; n=$scored.Count; ids=(@($ids) | Sort-Object) }
    Write-Host ("  {0,-45} Acc@5 {1,2}/{2}  Acc@10 {3,2}/{2}  ids@5: {4}" -f $name,$h5,$scored.Count,$h10,(@($agg[$name].ids) -join ','))
}

# ---- stop rule --------------------------------------------------------------------------
$fusedNames = @($variants.Keys | Where-Object { $_ -like 'fused*' })
$bestFused = $fusedNames | Sort-Object -Property @{Expression={ $agg[$_].h5 };Descending=$true}, @{Expression={ $agg[$_].h10 };Descending=$true}, @{Expression={ $_ };Descending=$false} | Select-Object -First 1
$bf = $agg[$bestFused]
$pct5  = [math]::Round(100.0*$bf.h5/$bf.n, 1)
$pct10 = [math]::Round(100.0*$bf.h10/$bf.n, 1)
$accOk   = ($pct5 -ge 56.0)
$buildOk = ($BuildMinutes -le 15.0)
$won = ($accOk -and $buildOk)
$verdict = if ($won) { 'VENCEU' } elseif (-not $buildOk) { 'PAROU (custo de build)' } else { 'PAROU' }

# ---- stress: 4 id15 sections through the BEST fused variant -----------------------------
$stressKey = $bf.key
$stress = @()
foreach ($sec in $sections) {
    $added = Added-Tokens $equiv $sec
    $qLex = ($sec + ' ' + ($added -join ' ')).Trim()
    $lex  = Run-Ranker $qLex
    $emb  = if ($stressKey -like '*En*') { Run-Embed $mtOf[$sec] } else { Run-Embed $sec }
    $k    = if ($stressKey -like '*60') { 60 } else { 20 }
    $fused = Fuse-RRF $lex $emb $k
    $stress += [ordered]@{ pt = $sec; en = $mtOf[$sec]; top5 = @($fused | Select-Object -First 5) }
    Write-Host "  stress: $($sec.Substring(0, [math]::Min(60,$sec.Length)))... -> $(@($fused | Select-Object -First 3) -join ' | ')"
}

# ---- report -----------------------------------------------------------------------------
$vecInfo = Get-Item -LiteralPath $Vectors
$sb = [Text.StringBuilder]::new()
$null=$sb.AppendLine('# Fusão léxico+denso (RRF) — o experimento FINAL da retrieval, com regra de parada')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("**Build do índice: $BuildMinutes min (critério ≤ 15 min: $(if ($buildOk) {'OK'} else {'ESTOUROU'})) · melhor fusão @5: $($bf.h5)/$($bf.n) ($pct5%) vs barra 46.2% · veredito: $verdict.**")
$null=$sb.AppendLine('')
$null=$sb.AppendLine("Generated $(Get-Date -Format 'yyyy-MM-dd HH:mm') by ``fused.ps1``. Ruler identical to the whole series: scored n=$($bf.n), hit = target OR secondary, exact path match. List A = ``grain rank`` (query = raw PT + equivalences-mt tokens, ``--direct-base $Base``, top $Depth). List B = ``mustard-embed search`` (multilingual-e5-SMALL, E5 ``query:``/``passage:`` prefixes, corpus gated to anchor-eligible non-test hand-written modules, bodies capped at 1000 chars ≈ 250 tokens, cosine over method-body vectors, top $Depth distinct files from $EmbedCandidates method candidates). Fusion = RRF ``score(f) = Σ 1/(k + rank)``. Vector sidecar: ``$(Split-Path -Leaf $Vectors)`` $([math]::Round($vecInfo.Length/1MB,1)) MB.")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## Acc@5 / Acc@10 (n=13)')
$null=$sb.AppendLine('')
$null=$sb.AppendLine('| Variant | Acc@5 | Acc@10 | hit ids @5 |')
$null=$sb.AppendLine('|---|---|---|---|')
foreach ($name in $agg.Keys) {
    $r = $agg[$name]
    $p5 = [math]::Round(100.0*$r.h5/$r.n,1); $p10 = [math]::Round(100.0*$r.h10/$r.n,1)
    $null=$sb.AppendLine("| $name | $($r.h5)/$($r.n) ($p5%) | $($r.h10)/$($r.n) ($p10%) | $(@($r.ids) -join ',') |")
}
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## REGRA DE PARADA')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("Melhor fusão: **$bestFused** = **$($bf.h5)/$($bf.n) ($pct5%) @5**, $($bf.h10)/$($bf.n) ($pct10%) @10, contra a barra léxica 6/13 (46.2%) @5 / 7/13 (53.8%) @10. Build do índice: **$BuildMinutes min** (critério: ≤ 15 min).")
$null=$sb.AppendLine('')
$null=$sb.AppendLine("**Veredito: $verdict** — regra: fusão ≥ 56% @5 (≥ 8/13) **E** build ≤ 15 min → VENCEU (vira a arquitetura final); qualquer um falhando → PAROU (a lista-curta 46/54 + salto do agente é a forma final). Grade fechada: 2 variantes de query do embed (PT, MT-EN) × 2 k de RRF (60, 20); nada além foi tentado.")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## Per-label best rank (target-or-secondary) — best fused vs its parents')
$null=$sb.AppendLine('')
$null=$sb.AppendLine('| id | lex | embed(best-variant) | fused(best) | hit@5 |')
$null=$sb.AppendLine('|---|---|---|---|---|')
$embKeyOfBest = if ($bf.key -like '*En*') { 'embEn' } else { 'embPt' }
foreach ($lab in $scored) {
    $id = [int]$lab.id; $p = $perId[$id]
    $rl = Best-Rank $p.lex $lab; $re = Best-Rank $p[$embKeyOfBest] $lab; $rf = Best-Rank $p[$bf.key] $lab
    $hit = if ($rf -ge 1 -and $rf -le 5) { 'Y' } else { '.' }
    $null=$sb.AppendLine("| $id | $rl | $re | $rf | $hit |")
}
$null=$sb.AppendLine('')
$null=$sb.AppendLine('(-1 = fora do top-'+$Depth+'; ranks são o MELHOR entre alvo e secundários.)')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("## Caso-estresse id15 — as 4 seções do prompt sialia-partners pela melhor fusão ($bestFused)")
$null=$sb.AppendLine('')
$null=$sb.AppendLine("Referências do gabarito id15: target ``$($lab15.target)``; secundários $(@($lab15.secondary | ForEach-Object { '``'+$_+'``' }) -join ', ').")
foreach ($s in $stress) {
    $null=$sb.AppendLine('')
    $null=$sb.AppendLine("### $($s.pt)")
    $null=$sb.AppendLine('')
    $null=$sb.AppendLine("MT-EN: ``$($s.en)``")
    $null=$sb.AppendLine('')
    $i=0; foreach ($f in $s.top5) { $i++; $null=$sb.AppendLine("$i. ``$f``") }
    $null=$sb.AppendLine('')
    $null=$sb.AppendLine('Avaliação: _(preenchida à mão após a run)_')
}
Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8

# raw sidecar for determinism auditing
[ordered]@{ perId = $perId; agg = $agg; stress = $stress; wallSeconds = [math]::Round($swAll.Elapsed.TotalSeconds,1) } |
    ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $RawPath -Encoding UTF8

Write-Host ''
Write-Host ("=== BEST FUSED: {0} -> {1}/{2} ({3}%) @5, {4}/{2} ({5}%) @10 — {6} ===" -f $bestFused,$bf.h5,$bf.n,$pct5,$bf.h10,$pct10,$verdict)
Write-Host "Wrote $OutPath"
Write-Host "Wrote $RawPath"
