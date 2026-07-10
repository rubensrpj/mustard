# pagerank-translated.ps1 - dict-seeded PageRank retrieval on LOCALLY-TRANSLATED queries (Wave 2c).
#
# The product ban: query translation may NOT come from a cloud LLM. This measures the
# replacement — `mustard-translate` (apps/translate: OPUS-MT ROMANCE->en via candle,
# greedy/deterministic, one-time ~300 MB download to the machine cache) — on the SAME
# `grain rank` ranker and the SAME regua as every previous run: scored labels only
# (n=13), target-OR-secondary within top-K, ranker default ungated direct seeding with
# `--direct-base 100000`.
#
# Pipeline per label: raw PT -> mustard-translate (local MT, EN out) -> append the
# equivalences.json domain overrides for the PT domain tokens present in the RAW PT
# (titulo->receivable etc.) -> `grain rank --query "<EN final>"`.
#
# Variants measured (all ungated, --direct-base 100000):
#   MT-EN only          : translated text alone (is local MT enough by itself?)
#   MT-EN + equiv       : the deliverable — translated text + domain overrides
#   gloss-EN + equiv    : labels.ndjson `en` (the Claude gloss) + overrides — same
#                         ranker/expansion, only the TRANSLATOR differs (MT-vs-Claude)
#   PT + MT-EN + equiv  : union query (keep the original, append the translation) —
#                         the product-shaped variant, comparable to aliased 46.2/53.8
#
# Read-only against the model+dict snapshots; native ConvertFrom-Json, no Python.

param(
    [string]$Exe          = (Join-Path $PSScriptRoot '..\..\target\debug\scan.exe'),
    [string]$TranslateExe = (Join-Path $PSScriptRoot '..\..\apps\translate\target\release\mustard-translate.exe'),
    [string]$Model        = (Join-Path $PSScriptRoot 'model\grain.model.json'),
    [string]$Dict         = (Join-Path $PSScriptRoot 'model\grain.dictionary.json'),
    [string]$EquivPath    = (Join-Path $PSScriptRoot 'equivalences.json'),
    [string]$LabelsPath   = (Join-Path $PSScriptRoot 'labels.ndjson'),
    [string]$OutPath      = (Join-Path $PSScriptRoot 'translated-results.md'),
    [string]$Base         = '100000'
)
$ErrorActionPreference = 'Stop'

# ---- accent-fold (same as pagerank-aliased.ps1) --------------------------------------
function Fold-Tok {
    param([string]$s)
    if ([string]::IsNullOrEmpty($s)) { return '' }
    $n = ($s.ToLowerInvariant()).Normalize([Text.NormalizationForm]::FormD)
    $sb = [System.Text.StringBuilder]::new()
    foreach ($c in $n.ToCharArray()) {
        if ([Globalization.CharUnicodeInfo]::GetUnicodeCategory($c) -ne [Globalization.UnicodeCategory]::NonSpacingMark) {
            $null = $sb.Append($c)
        }
    }
    return $sb.ToString().Normalize([Text.NormalizationForm]::FormC)
}

# ---- equivalences: folded-PT domain token -> English[] -------------------------------
$equivRaw = (Get-Content -Raw -LiteralPath $EquivPath | ConvertFrom-Json -Depth 64).equivalences
$equiv = @{}
foreach ($p in $equivRaw.PSObject.Properties) { $equiv[$p.Name] = @($p.Value) }

# English overrides for the PT domain tokens PRESENT in the raw PT (append-only).
function Get-Added {
    param([string]$pt)
    $added = [System.Collections.Generic.List[string]]::new()
    $seen  = @{}
    foreach ($tok in ($pt -split '[^\p{L}\p{Nd}]+')) {
        if ($tok.Length -lt 3) { continue }
        $f = Fold-Tok $tok
        if ($equiv.ContainsKey($f)) {
            foreach ($en in $equiv[$f]) {
                if (-not $seen.ContainsKey($en)) { $seen[$en] = $true; $added.Add($en) }
            }
        }
    }
    return $added
}

# ---- local MT (mustard-translate) ----------------------------------------------------
function Translate-Local {
    param([string]$pt)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $raw = & $TranslateExe text --input $pt 2>$null
    $sw.Stop()
    $line = @($raw) | Where-Object { $_ -match '^\{' } | Select-Object -Last 1
    if (-not $line) { return @{ en = $pt; detected = 'ERR'; ms = $sw.ElapsedMilliseconds } }
    try { $o = $line | ConvertFrom-Json -Depth 8 } catch { return @{ en = $pt; detected = 'ERR'; ms = $sw.ElapsedMilliseconds } }
    return @{ en = [string]$o.en; detected = [string]$o.detected; ms = $sw.ElapsedMilliseconds }
}

# ---- ranker + regua helpers (verbatim from pagerank-aliased.ps1) ---------------------
function Run-Ranker {
    param([string]$q, [int]$top = 10)
    if ([string]::IsNullOrWhiteSpace($q)) { return @() }
    $a = @('rank', $Model, '--dict', $Dict, '--query', $q, '--top', "$top", '--direct-base', $Base)
    $raw = & $Exe @a 2>$null | Out-String
    $idx = $raw.IndexOf('{'); if ($idx -lt 0) { return @() }
    try { $o = $raw.Substring($idx) | ConvertFrom-Json -Depth 64 } catch { return @() }
    return @($o.files | ForEach-Object { ([string]$_.file).Replace('\','/') })
}
function Find-Rank { param($files, [string]$t)
    if ([string]::IsNullOrWhiteSpace($t)) { return -2 }
    $t = $t.Replace('\','/'); for ($i=0;$i -lt $files.Count;$i++){ if ($files[$i] -eq $t){ return ($i+1) } } ; return -1 }
function HitK { param([int]$tr,[int[]]$sr,[int]$k)
    if ($tr -ge 1 -and $tr -le $k){ return $true }; foreach($r in @($sr)){ if($r -ge 1 -and $r -le $k){ return $true } }; return $false }
function Fmt-Rank { param([int]$r); switch ($r) { -2 { 'n/a' } -1 { 'miss' } default { "$r" } } }
function YN { param([bool]$b); if ($b) { 'Y' } else { '.' } }
function Pct { param([int]$k,[int]$t); if ($t -eq 0){ '0.0' } else { ('{0:N1}' -f (100.0*$k/$t)) } }

# ---- load labels ---------------------------------------------------------------------
$labels = @()
foreach ($line in (Get-Content -LiteralPath $LabelsPath)) { $t=$line.Trim(); if ($t.Length){ $labels += ($t | ConvertFrom-Json -Depth 64) } }
Write-Host "Loaded $($labels.Count) labels; equivalence keys: $($equiv.Count)"

# ---- warm the model (first call may download ~300 MB once), then translate ALL -------
Write-Host "warmup (may download the model on first ever run)..."
& $TranslateExe text --input 'aquecimento do modelo de tradução' *> $null
$tr = @{}
foreach ($lab in $labels) {
    $id = [int]$lab.id
    $tr[$id] = Translate-Local ([string]$lab.pt)
    Write-Host ("  id{0,-3} [{1}] {2} ms -> {3}" -f $id, $tr[$id].detected, $tr[$id].ms, $tr[$id].en)
}
$latencies = @($labels | ForEach-Object { $tr[[int]$_.id].ms })
$latAvg = [math]::Round(($latencies | Measure-Object -Average).Average)
$latMin = ($latencies | Measure-Object -Minimum).Minimum
$latMax = ($latencies | Measure-Object -Maximum).Maximum

# ---- variants ------------------------------------------------------------------------
$builders = [ordered]@{
    'MT-EN only (local translator)'  = { param($lab,$t,$added) $t.en }
    'MT-EN + equiv (deliverable)'    = { param($lab,$t,$added) ($t.en + ' ' + ($added -join ' ')).Trim() }
    'gloss-EN + equiv (Claude ref.)' = { param($lab,$t,$added) (([string]$lab.en) + ' ' + ($added -join ' ')).Trim() }
    'PT + MT-EN + equiv (union)'     = { param($lab,$t,$added) (([string]$lab.pt) + ' ' + $t.en + ' ' + ($added -join ' ')).Trim() }
}

function Eval-Variant {
    param([scriptblock]$build)
    $h5=0; $h10=0; $n=0; $ids=@(); $perid=@{}
    foreach ($lab in $labels) {
        if (-not $lab.scored) { continue }
        $n++
        $id = [int]$lab.id
        $added = Get-Added ([string]$lab.pt)
        $q = & $build $lab $tr[$id] $added
        $files = Run-Ranker $q
        $trk = Find-Rank $files ([string]$lab.target)
        $sr = @(); foreach($s in @($lab.secondary)){ $sr += (Find-Rank $files $s) }
        $hit5 = HitK $trk $sr 5
        if ($hit5){ $h5++; $ids += $id }
        if (HitK $trk $sr 10){ $h10++ }
        $perid[$id] = @{ rank=$trk; hit5=$hit5; hit10=(HitK $trk $sr 10); query=$q }
    }
    return @{ h5=$h5; h10=$h10; n=$n; ids=($ids | Sort-Object); perid=$perid }
}

$res = [ordered]@{}
foreach ($k in $builders.Keys) {
    Write-Host "  variant: $k ..."
    $res[$k] = Eval-Variant $builders[$k]
}
$mt    = $res['MT-EN only (local translator)']
$mtEq  = $res['MT-EN + equiv (deliverable)']
$glEq  = $res['gloss-EN + equiv (Claude ref.)']
$unEq  = $res['PT + MT-EN + equiv (union)']

# ---- id15 stress case: translation + top-5 under the deliverable query ---------------
$lab15 = $labels | Where-Object { [int]$_.id -eq 15 }
$added15 = Get-Added ([string]$lab15.pt)
$q15 = ($tr[15].en + ' ' + ($added15 -join ' ')).Trim()
$top15 = Run-Ranker $q15 10

# ---- diagnosis prose (finalized against the measured numbers) ------------------------
$DiagnosisText = @'
## Diagnosis (honest) — local MT is NOT a drop-in for the Claude gloss on this ranker; the loss is paraphrase-vs-identifier vocabulary, and EN-REPLACEMENT queries lose to raw-PT here under every translator

**Translator-vs-translator on the SAME ranker+expansion (the clean A/B): local MT LOSES.**
`MT-EN + equiv` 2/13 (15.4%) @5 / 5/13 (38.5%) @10 vs `gloss-EN + equiv` 5/13 (38.5%) @5.
The three @5 hits the gloss gets and MT does not are all NEAR-misses under MT+equiv (target
ranks: id13 6th, id2 7th, id8 9th), and each traces to one vocabulary slip:
- **id13**: MT renders the VERB — "where the bank statement **is reconciled**"; the gloss
  uses the NOUN "reconciliation" that names `ReconciliationService`. Conciliação/extrato
  themselves came out right.
- **id2**: MT hallucinates the parenthesised loanword "(aging)" into "(58)" — the one
  outright hallucination observed. The equivalences re-inject `aging` (rank 7), but the
  gloss also says "bar chart" → `aging-bar.tsx` top-5.
- **id8**: MT drops "enum" (says only "status"); the gloss's "status enum" names the C# file.
Other paraphrase slips that cost seeds: hook→"rendition" (id7), handler→"editor" (id12),
schema→"writing scheme" (id5), cadastro→"registry" (id3), grafico→"graph" (id2). OPUS-MT
base (2020, general-domain) translates the SENTENCE; retrieval needs the repo's identifier
NOUNS. The Claude gloss was written seeing the codebase vocabulary — a general-purpose MT
cannot recover that from the sentence alone. That is exactly where it loses.

**The domain stress words mostly survive:** vencimento→maturity (equivalences add
duedate/due), conciliação→reconciled, extrato bancário→bank statement,
fatura/título (EF)→"Invoice Entity/Title configuration (EF)" (genuinely good, id14);
desdobramento→"unfolding" (literal miss — but the user's own "(split)" plus the
equivalences' split/breakdown carry the concept into the query).

**EN-replacement is the wrong query shape for THIS ranker regardless of translator:** even
the Claude gloss + equiv scores 38.5% @5 vs raw-PT's 46.2% — the mined dictionary anchors
are substantially PT (sialia's comments), so replacing the PT text forfeits the dict-seed
route (id3/4/5/12 drop; id2/8/13 arrive). The digest engine is the mirror image (gloss
46.2 vs raw-PT 15.4 there). Translation must ADD to the PT query, never replace it.

**But adding MT output is not free either:** `PT + MT-EN + equiv` scores 30.8% @5 / 46.2%
@10 vs the aliased `PT + equiv` 46.2/53.8 — the extra paraphrase tokens (each an ungated
direct seed with a fan-in-exempt floor) promote sibling noise and push id3/id12 out of the
top-5. The ungated floor is noise-sensitive by construction; MT widens the seed set with
low-precision tokens.

**What the sidecar buys and what it does not:** the translator itself is sound —
deterministic (greedy, proven by a two-fresh-loads test), ~0.6-0.9 s/sentence as a cold
process on CPU, one-time 297.6 MB + 2.4 MB download to the machine cache, fail-open
(model missing → pass-through, exit 0), CC-BY-4.0. What it cannot do is guess identifier
vocabulary. Measured next steps: (a) use MT-EN where EN tokens are rewarded (digest-style
BM25) and as the bridge when no equivalence fires; (b) keep PT + equiv as the pagerank
headline query; (c) if MT enters the pagerank query, gate its tokens by dictionary
membership instead of the ungated floor — the seeding noise, not the translation, is what
breaks @5 in the union.
'@

# ---- render --------------------------------------------------------------------------
$sb = [System.Text.StringBuilder]::new()
$null=$sb.AppendLine("# Wave 2c - LOCAL MT (mustard-translate) vs the raw-PT / equiv / Claude-gloss réguas")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("Generated by ``benchmarks/sialia/pagerank-translated.ps1`` on $(Get-Date -Format 'yyyy-MM-dd HH:mm').")
$null=$sb.AppendLine("Translator under test: ``apps/translate`` (``mustard-translate``) — OPUS-MT ``Helsinki-NLP/opus-mt-ROMANCE-en`` (CC-BY-4.0) run locally with candle on CPU, GREEDY decode (deterministic), language detection via lingua {en,pt,es,fr}; weights 297.6 MB (safetensors, ``refs/pr/4``) + tokenizer 2.4 MB (Xenova mirror), downloaded ONCE to the machine cache (``LOCALAPPDATA\mustard-translate\models``). No cloud/LLM tokens anywhere on the query path.")
$null=$sb.AppendLine("Régua identical to all previous runs: scored labels (n=$($mtEq.n)), target-OR-secondary within top-K, ``grain rank`` ungated direct seeding, ``--direct-base $Base``.")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## Aggregate (scored labels only, n=$($mtEq.n))")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("| Retrieval | Acc@5 | Acc@10 | hit ids @5 |")
$null=$sb.AppendLine("|---|---|---|---|")
foreach ($k in $builders.Keys) {
    $r = $res[$k]
    $null=$sb.AppendLine("| $k | $($r.h5)/$($r.n) ($(Pct $r.h5 $r.n)%) | $($r.h10)/$($r.n) ($(Pct $r.h10 $r.n)%) | $(@($r.ids) -join ',') |")
}
$null=$sb.AppendLine("| — régua: dict-gated raw-PT (pre-fix) | 6/13 (46.2%) | 6/13 (46.2%) | 1,3,4,5,6,12 |")
$null=$sb.AppendLine("| — régua: UNGATED raw-PT + equiv (aliased) | 6/13 (46.2%) | 7/13 (53.8%) | 1,3,4,5,6,12 |")
$null=$sb.AppendLine("| — régua: digest gloss-Claude (PT -- EN, JUSTO) | 6/13 (46.2%) | 6/13 (46.2%) | 2,4,7,8,9,14 |")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("**Headline (honest): MT-EN + equiv $($mtEq.h5)/$($mtEq.n) ($(Pct $mtEq.h5 $mtEq.n)%) Acc@5 / $($mtEq.h10)/$($mtEq.n) ($(Pct $mtEq.h10 $mtEq.n)%) Acc@10 — BELOW the Claude-gloss reference on the same ranker ($($glEq.h5)/$($glEq.n) ($(Pct $glEq.h5 $glEq.n)%) @5) and below every raw-PT régua; the union PT + MT-EN + equiv reaches $($unEq.h5)/$($unEq.n) ($(Pct $unEq.h5 $unEq.n)%) @5 / $($unEq.h10)/$($unEq.n) ($(Pct $unEq.h10 $unEq.n)%) @10.**")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## Translation latency (per sentence, CPU, cold process each call)")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("avg $latAvg ms - min $latMin ms - max $latMax ms over the $($labels.Count) prompts (each call pays process spawn + 300 MB weight load + greedy decode; the model itself was already in the machine cache after the one-time download).")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## Per-prompt (detected language, latency, local translation, hits)")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("| id | diff | det | ms | MT@5 | MT+eq@5 | gloss+eq@5 | union@5 | rank(MT+eq) | local EN translation |")
$null=$sb.AppendLine("|---:|---|:---:|---:|:---:|:---:|:---:|:---:|:---:|---|")
foreach ($lab in $labels) {
    $id = [int]$lab.id
    $t = $tr[$id]
    $h  = @('.','.','.','.'); $rk = 'n/a'
    if ($lab.scored) {
        $h  = @( (YN $mt.perid[$id].hit5), (YN $mtEq.perid[$id].hit5), (YN $glEq.perid[$id].hit5), (YN $unEq.perid[$id].hit5) )
        $rk = Fmt-Rank ([int]$mtEq.perid[$id].rank)
    }
    $enTxt = ([string]$t.en) -replace '\|', '\|'
    $null=$sb.AppendLine(("| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7} | {8} | {9} |" -f `
        $id,[string]$lab.difficulty,$t.detected,$t.ms,$h[0],$h[1],$h[2],$h[3],$rk,$enTxt))
}
$null=$sb.AppendLine("")
$null=$sb.AppendLine("Note: id 10 & id 15 are ``scored:false`` (translated for the table, excluded from the aggregate).")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## The domain stress words (did vencimento/desdobramento/conciliação survive?)")
$null=$sb.AppendLine("")
foreach ($sid in @(9,11,13)) {
    $lab = $labels | Where-Object { [int]$_.id -eq $sid }
    $null=$sb.AppendLine("- id$sid PT: ``$([string]$lab.pt)``")
    $null=$sb.AppendLine("  - MT: ``$($tr[$sid].en)``")
}
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## id15 (stress, scored:false) — local translation + top-5 (MT-EN + equiv)")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("- PT: ``$([string]$lab15.pt)``")
$null=$sb.AppendLine("- MT ($($tr[15].ms) ms): ``$($tr[15].en)``")
$null=$sb.AppendLine("- top-5:")
$i = 0
foreach ($f in ($top15 | Select-Object -First 5)) { $i++; $null=$sb.AppendLine("  $i. ``$f``") }
$null=$sb.AppendLine("")
# movers vs the two pagerank réguas
$rawIds  = @(1,3,4,5,6,12)
$mtEqIds = @($mtEq.ids)
$gainVsRaw = @($mtEqIds | Where-Object { $rawIds -notcontains $_ })
$loseVsRaw = @($rawIds | Where-Object { $mtEqIds -notcontains $_ })
$null=$sb.AppendLine("## Movers (MT-EN + equiv vs dict-gated raw-PT 46.2%)")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("- Gained: " + $(if ($gainVsRaw.Count) { ($gainVsRaw | ForEach-Object { "id $_" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine("- Lost:   " + $(if ($loseVsRaw.Count) { ($loseVsRaw | ForEach-Object { "id $_" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine("")
$null=$sb.AppendLine($DiagnosisText)
$null=$sb.AppendLine("")
Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8

Write-Host ""
Write-Host "=== AGGREGATE (scored n=$($mtEq.n)) ==="
foreach ($k in $builders.Keys) { $r=$res[$k]; Write-Host ("  {0,-32} Acc@5 {1}/{2} ({3}%)  Acc@10 {4}/{2} ({5}%)  ids: {6}" -f $k,$r.h5,$r.n,(Pct $r.h5 $r.n),$r.h10,(Pct $r.h10 $r.n),(@($r.ids) -join ',')) }
Write-Host ("latency per sentence: avg {0} ms (min {1}, max {2})" -f $latAvg,$latMin,$latMax)
Write-Host "Wrote $OutPath"
