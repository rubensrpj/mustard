# compare-equiv.ps1 — which PT→EN equivalence source best feeds the ungated
# pagerank (C2 shape: query = raw PT + added EN tokens), ALL zero-Claude at
# generation AND query time except the historical `equivalences.json` control
# (Claude-authored, the bar to replace):
#
#   none    : raw PT only (no equivalences)
#   claude  : equivalences.json           — the Claude-authored bar (46.2/53.8)
#   mt      : equivalences-mt.json        — each dict term through `mustard-translate batch`
#                                           (local Marian; detected:en → no alias)
#   cooc    : equivalences-cooc.json      — DETERMINISTIC corpus co-occurrence: for each
#                                           comment-borne dict term, the top EN identifier
#                                           tokens of its anchor modules (tf-in-anchors ×
#                                           global-rarity), no model at all
#
# Generators run inline (cached to their json files; delete to regenerate).
# Read-only against everything but this folder.

param(
    [string]$Exe          = (Join-Path $PSScriptRoot '..\..\target\release\scan.exe'),
    [string]$TranslateExe = (Join-Path $PSScriptRoot '..\..\apps\translate\target\release\mustard-translate.exe'),
    [string]$Model        = (Join-Path $PSScriptRoot 'model\grain.model.json'),
    [string]$Dict         = (Join-Path $PSScriptRoot 'model\grain.dictionary.json'),
    [string]$LabelsPath   = (Join-Path $PSScriptRoot 'labels.ndjson'),
    [string]$OutPath      = (Join-Path $PSScriptRoot 'equiv-compare-results.md'),
    [string]$Base         = '100000',
    [int]$TopTokens       = 4
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Text.UTF8Encoding]::new($false)

function Fold-Tok { param([string]$s)
    if ([string]::IsNullOrEmpty($s)) { return '' }
    $n = ($s.ToLowerInvariant()).Normalize([Text.NormalizationForm]::FormD)
    $sb = [Text.StringBuilder]::new()
    foreach ($c in $n.ToCharArray()) { if ([Globalization.CharUnicodeInfo]::GetUnicodeCategory($c) -ne [Globalization.UnicodeCategory]::NonSpacingMark) { $null = $sb.Append($c) } }
    return $sb.ToString().Normalize([Text.NormalizationForm]::FormC)
}
# camelCase/PascalCase/snake splitter mirroring digest::tokenize's shape
function Split-Ident { param([string]$name)
    $parts = [regex]::Split($name, '(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])|[^A-Za-z0-9]+')
    return @($parts | ForEach-Object { $_.ToLowerInvariant() } | Where-Object { $_.Length -ge 3 -and $_ -match '[a-z]' })
}

$dictObj = Get-Content -Raw -LiteralPath $Dict | ConvertFrom-Json -Depth 64

# ---- generator: MT (local Marian, one batch spawn) -------------------------------------
$mtPath = Join-Path $PSScriptRoot 'equivalences-mt.json'
if (-not (Test-Path $mtPath)) {
    Write-Host "generating equivalences-mt.json (one mustard-translate batch over $($dictObj.terms.Count) terms)..."
    $terms = @($dictObj.terms | ForEach-Object { $_.term })
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $rawOut = ($terms -join "`n") | & $TranslateExe batch 2>$null
    $sw.Stop()
    $jsonLines = @($rawOut | Where-Object { $_ -match '^\{' })
    if ($jsonLines.Count -ne $terms.Count) { throw "mt batch contract broken: $($jsonLines.Count)/$($terms.Count)" }
    $map = [ordered]@{}
    for ($i = 0; $i -lt $terms.Count; $i++) {
        $o = $jsonLines[$i] | ConvertFrom-Json -Depth 8
        if ($o.detected -eq 'en') { continue }                       # already English — no alias needed
        $toks = @(Split-Ident ([string]$o.en) | Where-Object { $_ -ne (Fold-Tok $terms[$i]) } | Select-Object -Unique -First $TopTokens)
        if ($toks.Count) { $map[(Fold-Tok $terms[$i])] = $toks }
    }
    @{ equivalences = $map } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $mtPath -Encoding UTF8
    Write-Host ("  {0} aliased terms in {1}s" -f $map.Count, [math]::Round($sw.Elapsed.TotalSeconds,1))
}

# ---- generator: co-occurrence (deterministic, no model at all) -------------------------
$coocPath = Join-Path $PSScriptRoot 'equivalences-cooc.json'
if (-not (Test-Path $coocPath)) {
    Write-Host 'generating equivalences-cooc.json (corpus co-occurrence, no model)...'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $modelObj = Get-Content -Raw -LiteralPath $Model | ConvertFrom-Json -Depth 64
    # token → set of module paths (df) + per-module identifier token bags
    $dfMap = @{}; $modTokens = @{}
    foreach ($m in $modelObj.modules) {
        $bag = @{}
        foreach ($d in @($m.declarations)) { foreach ($t in (Split-Ident ([string]$d.name))) { $bag[$t] = [int]$bag[$t] + 1 } }
        if ($bag.Count) {
            $modTokens[[string]$m.path] = $bag
            foreach ($t in $bag.Keys) { $dfMap[$t] = [int]$dfMap[$t] + 1 }
        }
    }
    $nDocs = [math]::Max(1, $modTokens.Count)
    $map = [ordered]@{}
    foreach ($e in $dictObj.terms) {
        if ($e.source -eq 'ident') { continue }                      # identifier terms ARE code vocabulary already
        $fold = Fold-Tok ([string]$e.term)
        if ($fold -match '^[a-z]+$' -and $e.source -eq 'both') { continue }  # EN-looking, already in identifiers
        $cand = @{}
        foreach ($a in @($e.anchors)) {
            $bag = $modTokens[[string]$a]; if ($null -eq $bag) { continue }
            foreach ($t in $bag.Keys) { $cand[$t] = [int]$cand[$t] + [int]$bag[$t] }
        }
        if (-not $cand.Count) { continue }
        $scored = foreach ($t in $cand.Keys) {
            $df = [math]::Max(1, [int]$dfMap[$t])
            if ($df * 2 -gt $nDocs) { continue }                     # ubiquity ceiling, same rule as the dict
            if ($t -eq $fold) { continue }
            [pscustomobject]@{ tok = $t; score = [double]$cand[$t] * [math]::Log($nDocs / $df) }
        }
        $top = @($scored | Sort-Object -Property @{Expression='score';Descending=$true}, @{Expression='tok';Descending=$false} | Select-Object -First $TopTokens | ForEach-Object { $_.tok })
        if ($top.Count) { $map[$fold] = $top }
    }
    $sw.Stop()
    @{ equivalences = $map } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $coocPath -Encoding UTF8
    Write-Host ("  {0} aliased terms in {1}s" -f $map.Count, [math]::Round($sw.Elapsed.TotalSeconds,1))
}

# ---- the C2-shape evaluation over each equivalence source ------------------------------
function Load-Equiv { param([string]$p)
    if ([string]::IsNullOrEmpty($p)) { return @{} }
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
function Run-Ranker { param([string]$q)
    if ([string]::IsNullOrWhiteSpace($q)) { return @() }
    $raw = & $Exe rank $Model --dict $Dict --query $q --top 10 --direct-base $Base 2>$null | Out-String
    $idx = $raw.IndexOf('{'); if ($idx -lt 0) { return @() }
    try { $o = $raw.Substring($idx) | ConvertFrom-Json -Depth 64 } catch { return @() }
    return @($o.files | ForEach-Object { ([string]$_.file).Replace('\','/') })
}
function Find-Rank { param($files, [string]$t)
    if ([string]::IsNullOrWhiteSpace($t)) { return -2 }
    $t = $t.Replace('\','/'); for ($i=0;$i -lt $files.Count;$i++){ if ($files[$i] -eq $t){ return ($i+1) } }; return -1 }
function HitK { param([int]$tr,[int[]]$sr,[int]$k)
    if ($tr -ge 1 -and $tr -le $k){ return $true }; foreach($r in @($sr)){ if($r -ge 1 -and $r -le $k){ return $true } }; return $false }

$labels = @(); foreach ($line in (Get-Content -LiteralPath $LabelsPath)) { $t=$line.Trim(); if ($t.Length){ $labels += ($t | ConvertFrom-Json -Depth 64) } }

$mtEq = Load-Equiv $mtPath
$coocEq = Load-Equiv $coocPath
# hybrid: per-term UNION — the MT gives the English WORD, the co-occurrence gives
# the REPO's word (cliente → customer ∪ client); cached for the product.
$hybrid = @{}
foreach ($k in @($mtEq.Keys) + @($coocEq.Keys) | Select-Object -Unique) {
    $hybrid[$k] = @(@($mtEq[$k]) + @($coocEq[$k]) | Where-Object { $_ } | Select-Object -Unique)
}
$hybridOut = [ordered]@{}; foreach ($k in ($hybrid.Keys | Sort-Object)) { $hybridOut[$k] = $hybrid[$k] }
@{ equivalences = $hybridOut } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'equivalences-hybrid.json') -Encoding UTF8

$sources = [ordered]@{
    'none (raw PT)'      = @{}
    'claude (the bar)'   = Load-Equiv (Join-Path $PSScriptRoot 'equivalences.json')
    'mt (local Marian)'  = $mtEq
    'cooc (no model)'    = $coocEq
    'hybrid (mt+cooc)'   = $hybrid
}
$rows = [ordered]@{}
foreach ($name in $sources.Keys) {
    $eq = $sources[$name]; $h5=0; $h10=0; $n=0; $ids=@()
    foreach ($lab in $labels) {
        if (-not $lab.scored) { continue }
        $n++
        $added = Added-Tokens $eq ([string]$lab.pt)
        $q = (([string]$lab.pt) + ' ' + ($added -join ' ')).Trim()
        $files = Run-Ranker $q
        $trk = Find-Rank $files ([string]$lab.target)
        $sr = @(); foreach($s in @($lab.secondary)){ $sr += (Find-Rank $files $s) }
        if (HitK $trk $sr 5)  { $h5++; $ids += [int]$lab.id }
        if (HitK $trk $sr 10) { $h10++ }
    }
    $rows[$name] = @{ h5=$h5; h10=$h10; n=$n; ids=(@($ids) | Sort-Object) }
    Write-Host ("  {0,-20} Acc@5 {1}/{2}  Acc@10 {3}/{2}  ids: {4}" -f $name,$h5,$n,$h10,(@($rows[$name].ids) -join ','))
}

$sb = [Text.StringBuilder]::new()
$null=$sb.AppendLine('# Equivalence sources on the ungated pagerank (C2 shape) — zero-Claude generation')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("Generated $(Get-Date -Format 'yyyy-MM-dd HH:mm') by ``compare-equiv.ps1``. Model+dict: the post-revert scan (raw PT dictionary back; scan wall-time ~27 s on sialia). Query = raw ``pt`` + the source's added EN tokens; ``grain rank --direct-base $Base``; scored n=13, target-OR-secondary.")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('| Equivalence source | Acc@5 | Acc@10 | hit ids @5 |')
$null=$sb.AppendLine('|---|---|---|---|')
foreach ($name in $rows.Keys) { $r=$rows[$name]; $null=$sb.AppendLine("| $name | $($r.h5)/$($r.n) | $($r.h10)/$($r.n) | $(@($r.ids) -join ',') |") }
$null=$sb.AppendLine('')
$null=$sb.AppendLine('Bars: claude-authored equivalences on the pre-revert pair measured 6/13 @5, 7/13 @10.')
Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8
Write-Host "Wrote $OutPath"
