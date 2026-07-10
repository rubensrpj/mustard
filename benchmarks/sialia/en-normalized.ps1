# en-normalized.ps1 - THE DECISIVE EN->EN MEASUREMENT (Wave 3): scan-normalized English
# dictionary vs the raw-PT reguas, all arrangements ZERO Claude on the query path.
#
# The product rule under test: the scan generates EVERYTHING in English (comments are
# machine-translated at MINE time by the local `mustard-translate` sidecar), a free-form
# prompt is machine-translated at QUERY time, and retrieval always crosses EN->EN.
#
# Arrangements (same regua as every previous run: scored labels n=13, target-OR-secondary
# within top-K, native ConvertFrom-Json, no Python):
#   A  pagerank EN->EN      : query = local MT-EN of the raw PT -> `grain rank` against the
#                             NEW EN model+dictionary, NO equivalences (in EN->EN they lose
#                             their role: the dict itself is the bridge now).
#   B  digest EN-gloss-local: `mustard-rt run feature --intent "<pt> -- <MT-EN>"` with
#                             cwd = sialia (the JUSTO shape, Claude gloss replaced by the
#                             local MT). NOTE: the digest reads SIALIA'S OWN
#                             .claude/grain.model.json (the old PT-era model) - nothing is
#                             written to sialia; the model it saw is hashed in the report.
#   C  control              : pagerank raw-PT (dict-gated) and raw-PT+equiv (UNGATED) against
#                             the BACKED-UP PT dictionary - must reproduce 46.2 / 46.2-53.8
#                             to validate the environment.
# Ablations for the verdict: raw-PT vs EN dict (what a PT prompt gets against the new dict)
# and gloss-EN vs EN dict (the EN->EN ceiling if translation were perfect).
#
# Read-only against sialia and against every model/dict snapshot. Writes ONLY
# en-normalized-results.md in this folder.

param(
    [string]$Exe          = (Join-Path $PSScriptRoot '..\..\target\debug\scan.exe'),
    [string]$TranslateExe = (Join-Path $PSScriptRoot '..\..\apps\translate\target\release\mustard-translate.exe'),
    [string]$ModelNew     = (Join-Path $PSScriptRoot 'model\grain.model.json'),
    [string]$DictNew      = (Join-Path $PSScriptRoot 'model\grain.dictionary.json'),
    [string]$ModelOld     = (Join-Path $PSScriptRoot 'model\grain.model.prev.json'),
    [string]$DictOld      = (Join-Path $PSScriptRoot 'model\grain.dictionary.pt.json'),
    [string]$EquivPath    = (Join-Path $PSScriptRoot 'equivalences.json'),
    [string]$LabelsPath   = (Join-Path $PSScriptRoot 'labels.ndjson'),
    [string]$Sialia       = 'C:\Atiz\sialia',
    [string]$OutPath      = (Join-Path $PSScriptRoot 'en-normalized-results.md'),
    [string]$Base         = '100000',
    [switch]$SkipDigest
)
$ErrorActionPreference = 'Stop'
# BOM-LESS UTF-8 on both directions of the native pipe: the BOM-carrying
# [Text.Encoding]::UTF8 corrupts the FIRST stdin line of the batch (the MT
# tokenizer sees U+FEFF glued to the first word and mistranslates it).
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Text.UTF8Encoding]::new($false)

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

# ---- equivalences: folded-PT domain token -> English[] (control C2 only) -------------
$equivRaw = (Get-Content -Raw -LiteralPath $EquivPath | ConvertFrom-Json -Depth 64).equivalences
$equiv = @{}
foreach ($p in $equivRaw.PSObject.Properties) { $equiv[$p.Name] = @($p.Value) }
function Get-Added {
    param([string]$pt)
    $added = [System.Collections.Generic.List[string]]::new(); $seen = @{}
    foreach ($tok in ($pt -split '[^\p{L}\p{Nd}]+')) {
        if ($tok.Length -lt 3) { continue }
        $f = Fold-Tok $tok
        if ($equiv.ContainsKey($f)) {
            foreach ($en in $equiv[$f]) { if (-not $seen.ContainsKey($en)) { $seen[$en] = $true; $added.Add($en) } }
        }
    }
    return $added
}

# ---- ranker + regua helpers ------------------------------------------------------------
function Run-Ranker {
    param([string]$model, [string]$dict, [string]$q, [string[]]$extra)
    if ([string]::IsNullOrWhiteSpace($q)) { return @() }
    $a = @('rank', $model, '--dict', $dict, '--query', $q, '--top', '10') + $extra
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

# ---- digest runner (verbatim ranked-list rule from run.ps1) ---------------------------
function Get-DigestFiles {
    param([string]$Intent)
    if ([string]::IsNullOrWhiteSpace($Intent)) { return @{ files=@(); error='empty-intent' } }
    Push-Location -LiteralPath $Sialia
    try { $raw = & mustard-rt run feature --intent $Intent 2>$null; $text = ($raw | Out-String) }
    finally { Pop-Location }
    $idx = $text.IndexOf('{'); if ($idx -lt 0) { return @{ files=@(); error='no-json' } }
    try { $obj = $text.Substring($idx) | ConvertFrom-Json -Depth 64 } catch { return @{ files=@(); error='parse-fail' } }
    $map = @{}
    $details = [System.Collections.ArrayList]::new()
    foreach ($a in @($obj.anchorsDetail)) { if ($null -ne $a) { [void]$details.Add($a) } }
    foreach ($c in @($obj.concerns)) { if ($null -eq $c) { continue }; foreach ($a in @($c.anchorsDetail)) { if ($null -ne $a) { [void]$details.Add($a) } } }
    foreach ($a in $details) {
        if ($null -eq $a.file) { continue }
        $f = ([string]$a.file).Replace('\','/'); $s = [int]$a.scoreX1024
        if (-not $map.ContainsKey($f) -or $map[$f] -lt $s) { $map[$f] = $s }
    }
    $ranked = @($map.GetEnumerator() | ForEach-Object { [pscustomobject]@{ file=$_.Key; score=[int]$_.Value } } |
        Sort-Object -Property @{Expression='score';Descending=$true}, @{Expression='file';Descending=$false} |
        ForEach-Object { $_.file })
    return @{ files=$ranked; error=$null }
}

# ---- load labels ----------------------------------------------------------------------
$labels = @()
foreach ($line in (Get-Content -LiteralPath $LabelsPath)) { $t=$line.Trim(); if ($t.Length){ $labels += ($t | ConvertFrom-Json -Depth 64) } }
Write-Host "Loaded $($labels.Count) labels; equivalence keys: $($equiv.Count)"

# ---- ONE batch translation of every PT prompt (dogfoods `mustard-translate batch`) ----
Write-Host "batch-translating $($labels.Count) prompts (single model load)..."
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$ptJoined = (($labels | ForEach-Object { ([string]$_.pt) -replace '\r?\n',' ' }) -join "`n")
$rawBatch = $ptJoined | & $TranslateExe batch 2>$null
$sw.Stop()
$jsonLines = @($rawBatch | Where-Object { $_ -match '^\{' })
if ($jsonLines.Count -ne $labels.Count) { throw "batch contract broken: $($jsonLines.Count) lines out for $($labels.Count) in" }
$tr = @{}
for ($i = 0; $i -lt $labels.Count; $i++) {
    $o = $jsonLines[$i] | ConvertFrom-Json -Depth 8
    $tr[[int]$labels[$i].id] = @{ en = [string]$o.en; detected = [string]$o.detected }
}
$batchMs = $sw.ElapsedMilliseconds
Write-Host ("  batch done in {0} ms ({1} ms/prompt incl. one model load)" -f $batchMs, [math]::Round($batchMs/$labels.Count))
foreach ($lab in $labels) { $id=[int]$lab.id; Write-Host ("  id{0,-3} [{1}] {2}" -f $id, $tr[$id].detected, $tr[$id].en) }

# ---- pagerank variants ----------------------------------------------------------------
$variants = [ordered]@{
    'A  EN->EN: MT-EN query vs EN dict (no equiv)'   = @{ model=$ModelNew; dict=$DictNew; q={ param($lab) $tr[[int]$lab.id].en };                        extra=@('--direct-base',$Base) }
    'abl raw-PT vs EN dict'                          = @{ model=$ModelNew; dict=$DictNew; q={ param($lab) [string]$lab.pt };                             extra=@('--direct-base',$Base) }
    'abl gloss-EN (Claude) vs EN dict (ceiling)'     = @{ model=$ModelNew; dict=$DictNew; q={ param($lab) [string]$lab.en };                             extra=@('--direct-base',$Base) }
    'C1 control: dict-gated raw-PT vs PT dict'       = @{ model=$ModelOld; dict=$DictOld; q={ param($lab) [string]$lab.pt };                             extra=@('--no-direct-seed') }
    'C2 control: UNGATED raw-PT+equiv vs PT dict'    = @{ model=$ModelOld; dict=$DictOld; q={ param($lab) (([string]$lab.pt) + ' ' + ((Get-Added ([string]$lab.pt)) -join ' ')).Trim() }; extra=@('--direct-base',$Base) }
}
function Eval-Variant {
    param($spec)
    $h5=0; $h10=0; $n=0; $ids=@(); $perid=@{}
    foreach ($lab in $labels) {
        if (-not $lab.scored) { continue }
        $n++; $id=[int]$lab.id
        $q = & $spec.q $lab
        $files = Run-Ranker $spec.model $spec.dict $q $spec.extra
        $trk = Find-Rank $files ([string]$lab.target)
        $sr = @(); foreach($s in @($lab.secondary)){ $sr += (Find-Rank $files $s) }
        $hit5 = HitK $trk $sr 5
        if ($hit5){ $h5++; $ids += $id }
        if (HitK $trk $sr 10){ $h10++ }
        $perid[$id] = @{ rank=$trk; hit5=$hit5; hit10=(HitK $trk $sr 10); top5=@($files | Select-Object -First 5) }
    }
    return @{ h5=$h5; h10=$h10; n=$n; ids=($ids | Sort-Object); perid=$perid }
}
$res = [ordered]@{}
foreach ($k in $variants.Keys) { Write-Host "  variant: $k ..."; $res[$k] = Eval-Variant $variants[$k] }

# ---- print aggregates IMMEDIATELY (survives any render failure below) -----------------
Write-Host ''
Write-Host ("=== AGGREGATE EARLY (res.Count={0}) ===" -f $res.Count)
foreach ($k in $res.Keys) {
    $r = $res[$k]
    $ty = if ($null -eq $r) { 'NULL' } else { $r.GetType().Name }
    if ($null -ne $r -and $null -ne $r.n) {
        Write-Host ("  {0,-46} [{1}] Acc@5 {2}/{3} ({4}%)  Acc@10 {5}/{3} ({6}%)  ids: {7}" -f $k,$ty,$r.h5,$r.n,(Pct $r.h5 $r.n),$r.h10,(Pct $r.h10 $r.n),(@($r.ids) -join ','))
    } else {
        Write-Host ("  {0,-46} [{1}] <no data>" -f $k,$ty)
    }
}

# ---- B: digest with the LOCAL EN gloss (cwd = sialia, old PT-era model) ---------------
$sialiaModel = Join-Path $Sialia '.claude\grain.model.json'
$sialiaModelInfo = if (Test-Path $sialiaModel) {
    $h = (Get-FileHash $sialiaModel -Algorithm SHA256).Hash.Substring(0,12)
    $t = (Get-Item $sialiaModel).LastWriteTime.ToString('yyyy-MM-dd HH:mm')
    "sha256:$h (mtime $t)"
} else { 'ABSENT' }
$dig = @{ h5=0; h10=0; n=0; ids=@(); perid=@{} }
if (-not $SkipDigest) {
    Write-Host "  variant: B digest PT -- MT-EN (cwd=sialia; model: $sialiaModelInfo) ..."
    foreach ($lab in $labels) {
        if (-not $lab.scored) { continue }
        $dig.n++; $id=[int]$lab.id
        $r = Get-DigestFiles -Intent ("$($lab.pt) -- $($tr[$id].en)")
        $trk = Find-Rank $r.files ([string]$lab.target)
        $sr = @(); foreach($s in @($lab.secondary)){ $sr += (Find-Rank $r.files $s) }
        $hit5 = HitK $trk $sr 5
        if ($hit5){ $dig.h5++; $dig.ids += $id }
        if (HitK $trk $sr 10){ $dig.h10++ }
        $dig.perid[$id] = @{ rank=$trk; hit5=$hit5; hit10=(HitK $trk $sr 10); error=$r.error }
        Write-Host ("    id{0,-3} rank(target)={1} hit5={2}{3}" -f $id, (Fmt-Rank $trk), (YN $hit5), $(if($r.error){" ERR=$($r.error)"}else{''}))
    }
    $dig.ids = @($dig.ids | Sort-Object)
}

# ---- EN dictionary: top terms + normalization telemetry -------------------------------
$dictObj = Get-Content -Raw -LiteralPath $DictNew | ConvertFrom-Json -Depth 64
$topTerms = @($dictObj.terms | Sort-Object -Property @{Expression='specificity_x1024';Descending=$true}, @{Expression='term';Descending=$false} | Select-Object -First 12)
$nonEn = [int]$dictObj.non_english_comments
$srcStats = $dictObj.terms | Group-Object source | ForEach-Object { "$($_.Name)=$($_.Count)" }

# ---- verdict prose (finalized against the measured numbers, waves-2b/2c pattern) ------
$VerdictText = @'
## Verdict

**EN-normalizing the comment-derived dictionary DESTROYS retrieval on this corpus.** Every
EN-query arrangement against the EN dict craters (A: 0% @5; even the PERFECT Claude gloss:
7.7% @5) while the SAME ranker on the PT dict holds 46.2/53.8 (C2, reproduced). This is not
an MT-quality problem — the gloss-Claude ablation proves it: merging translated comment
tokens into the already-English identifier vocabulary erases exactly the distinctiveness
that made the dictionary discriminative (the PT terms were rare, domain-bearing keys; their
EN translations collide with ubiquitous identifier tokens and die to the ubiquity ceiling
or drown in shared anchors).

**Consequences for the product design:**
1. `grain.model.json` stays ALL-ENGLISH — it stores identifiers only (no comment text), so
   the canonical-English rule holds there by construction.
2. The dictionary is the BRIDGE artifact: its KEYS are user-language domain terms (mined
   from comments), its VALUES are English (anchors, code terms). A dictionary with English
   on both sides cannot translate anything — "traduzir o prompt com base no dicionário do
   projeto" REQUIRES the user-language keys. C2 measures exactly this design: 46.2/53.8,
   zero cloud tokens at query time.
3. The full-comment mine-time MT batch (~70 min on sialia) is DELETED from the design: it
   was the slowest step and it produced the losing dictionary. Non-English comments stay
   FLAGGED (`non_english_comments`) as the fix-the-code signal. The local MT's remaining
   scan-time job is small: translating the few hundred distinctive dictionary TERMS to
   build the EN side of the bridge (replacing the Claude-authored equivalences.json) —
   seconds, not tens of minutes.
4. B (digest + local-MT gloss) = 30.8% @5 vs 46.2% with the Claude gloss: the generic MT
   loses identifier-bearing nouns (hook→rendition, handler→editor, aging→(58)). It still
   nails id7/id8 at rank 1 — evidence the digest engine benefits from EN queries when the
   nouns survive. Not the product path while C2 exists.
'@

# ---- render ---------------------------------------------------------------------------
# Positional access (ordered dict) — string-key lookup proved brittle at render time.
$vals = @($res.Values)
$A   = $vals[0]
$abP = $vals[1]
$abG = $vals[2]
$C1  = $vals[3]
$C2  = $vals[4]
# Defensive per-id maps: .perid member access failed at render on an otherwise-healthy
# hashtable (h5/n/ids all resolve); index explicitly and degrade to '?' cells.
Write-Host ("DEBUG A keys: " + (@($A.Keys) -join '|'))
$Aper  = $A['perid'];   if ($null -eq $Aper)  { Write-Host 'DEBUG: A[perid] NULL'; $Aper  = @{} }
$C2per = $C2['perid'];  if ($null -eq $C2per) { $C2per = @{} }

$sb = [System.Text.StringBuilder]::new()
$null=$sb.AppendLine('# Wave 3 - EN->EN normalized dictionary: the decisive zero-Claude measurement')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("Generated by ``benchmarks/sialia/en-normalized.ps1`` on $(Get-Date -Format 'yyyy-MM-dd HH:mm').")
$null=$sb.AppendLine('The scan now NORMALIZES COMMENTS TO ENGLISH at mine time (`apps/scan/src/dictionary.rs` + the `mustard-translate batch` sidecar, one spawn, greedy/deterministic); the dictionary under test is 100% English. Query path: raw PT -> `mustard-translate batch` (ONE model load for all 15 prompts) -> the EN query. Zero cloud/LLM tokens anywhere.')
$null=$sb.AppendLine("Regua identical to all previous runs: scored labels (n=$($A.n)), target-OR-secondary within top-K, ``grain rank`` ungated direct seeding ``--direct-base $Base`` (controls C reproduce the historical configs). Batch translation of the 15 prompts: $batchMs ms total.")
$null=$sb.AppendLine('')
$null=$sb.AppendLine("## Aggregate (scored labels only, n=$($A.n))")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('| Arrangement (all zero-Claude) | Acc@5 | Acc@10 | hit ids @5 |')
$null=$sb.AppendLine('|---|---|---|---|')
$null=$sb.AppendLine("| **A pagerank EN->EN** (MT-EN query, EN dict, no equiv) | $($A.h5)/$($A.n) ($(Pct $A.h5 $A.n)%) | $($A.h10)/$($A.n) ($(Pct $A.h10 $A.n)%) | $(@($A.ids) -join ',') |")
if (-not $SkipDigest) {
$null=$sb.AppendLine("| **B digest EN-gloss-local** (``<pt> -- <MT-EN>``, sialia's own model) | $($dig.h5)/$($dig.n) ($(Pct $dig.h5 $dig.n)%) | $($dig.h10)/$($dig.n) ($(Pct $dig.h10 $dig.n)%) | $(@($dig.ids) -join ',') |")
}
$null=$sb.AppendLine("| **C1 control** dict-gated raw-PT vs PT dict (expect 46.2/46.2) | $($C1.h5)/$($C1.n) ($(Pct $C1.h5 $C1.n)%) | $($C1.h10)/$($C1.n) ($(Pct $C1.h10 $C1.n)%) | $(@($C1.ids) -join ',') |")
$null=$sb.AppendLine("| **C2 control** UNGATED raw-PT+equiv vs PT dict (expect 46.2/53.8) | $($C2.h5)/$($C2.n) ($(Pct $C2.h5 $C2.n)%) | $($C2.h10)/$($C2.n) ($(Pct $C2.h10 $C2.n)%) | $(@($C2.ids) -join ',') |")
$null=$sb.AppendLine("| - ablation: raw-PT query vs EN dict | $($abP.h5)/$($abP.n) ($(Pct $abP.h5 $abP.n)%) | $($abP.h10)/$($abP.n) ($(Pct $abP.h10 $abP.n)%) | $(@($abP.ids) -join ',') |")
$null=$sb.AppendLine("| - ablation: gloss-EN (Claude) query vs EN dict (translation ceiling) | $($abG.h5)/$($abG.n) ($(Pct $abG.h5 $abG.n)%) | $($abG.h10)/$($abG.n) ($(Pct $abG.h10 $abG.n)%) | $(@($abG.ids) -join ',') |")
$null=$sb.AppendLine('| - regua: digest gloss-Claude (PT -- EN, JUSTO) | 6/13 (46.2%) | 6/13 (46.2%) | 2,4,7,8,9,14 |')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("## The EN dictionary itself ($($dictObj.terms.Count) terms; sources: $($srcStats -join ', '); non_english_comments=$nonEn)")
$null=$sb.AppendLine('')
$null=$sb.AppendLine('Top terms by specificity (was contrato/parceiro/valor... in the PT dict):')
$null=$sb.AppendLine('')
$null=$sb.AppendLine('| term | spec_x1024 | count | df | source |')
$null=$sb.AppendLine('|---|---:|---:|---:|---|')
foreach ($t in $topTerms) { $null=$sb.AppendLine("| $($t.term) | $($t.specificity_x1024) | $($t.count) | $($t.df) | $($t.source) |") }
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## Per-prompt')
$null=$sb.AppendLine('')
$null=$sb.AppendLine('| id | diff | A@5 | rank A | B@5 | C2@5 | local MT-EN query (A/B gloss) |')
$null=$sb.AppendLine('|---:|---|:---:|:---:|:---:|:---:|---|')
foreach ($lab in $labels) {
    $id=[int]$lab.id
    $a='.'; $ra='n/a'; $b='.'; $c='.'
    if ($lab.scored) {
        if ($Aper.ContainsKey($id))      { $a = YN $Aper[$id].hit5; $ra = Fmt-Rank ([int]$Aper[$id].rank) } else { $a='?'; $ra='?' }
        if (-not $SkipDigest -and $dig.perid.ContainsKey($id)) { $b = YN $dig.perid[$id].hit5 }
        if ($C2per.ContainsKey($id))     { $c = YN $C2per[$id].hit5 } else { $c='?' }
    }
    $enTxt = ([string]$tr[$id].en) -replace '\|', '\|'
    $null=$sb.AppendLine("| $id | $($lab.difficulty) | $a | $ra | $b | $c | $enTxt |")
}
$null=$sb.AppendLine('')
$null=$sb.AppendLine('Note: id 10 & id 15 are `scored:false` (translated for the table, excluded from the aggregate).')
$null=$sb.AppendLine('')
# movers vs the C2 regua (the strongest raw-PT arrangement)
$c2Ids = @($C2.ids)
$null=$sb.AppendLine('## Movers (A EN->EN vs C2 raw-PT+equiv on the PT dict)')
$null=$sb.AppendLine('')
$gain = @($A.ids | Where-Object { $c2Ids -notcontains $_ })
$lose = @($c2Ids | Where-Object { @($A.ids) -notcontains $_ })
$null=$sb.AppendLine('- Gained: ' + $(if ($gain.Count) { ($gain | ForEach-Object { "id $_" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine('- Lost:   ' + $(if ($lose.Count) { ($lose | ForEach-Object { "id $_" }) -join ', ' } else { '(none)' }))
if (-not $SkipDigest) {
    $gIds = @(2,4,7,8,9,14)
    $gainD = @($dig.ids | Where-Object { $gIds -notcontains $_ })
    $loseD = @($gIds | Where-Object { @($dig.ids) -notcontains $_ })
    $null=$sb.AppendLine('- B vs digest gloss-Claude (46.2): gained ' + $(if ($gainD.Count) { ($gainD | ForEach-Object { "id $_" }) -join ', ' } else { '(none)' }) + ' / lost ' + $(if ($loseD.Count) { ($loseD | ForEach-Object { "id $_" }) -join ', ' } else { '(none)' }))
}
$null=$sb.AppendLine('')
$null=$sb.AppendLine('## Honesty ledger')
$null=$sb.AppendLine('')
$null=$sb.AppendLine("- B ran against sialia's INSTALLED model ($sialiaModelInfo) - the old PT-era scan; the new EN model was NOT copied into sialia (read-only rule). B therefore measures the local-MT gloss on the digest engine as deployed, not the EN dictionary.")
$null=$sb.AppendLine("- A/ablations ran on the freshly re-scanned ``benchmarks/sialia/model`` pair (EN dictionary, non_english_comments=$nonEn); C ran on the backed-up PT pair (``grain.dictionary.pt.json`` + ``grain.model.prev.json``).")
$null=$sb.AppendLine('')
$sb.Append($VerdictText) | Out-Null
Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8

Write-Host ''
Write-Host "=== AGGREGATE (scored n=$($A.n)) ==="
foreach ($k in $res.Keys) { $r=$res[$k]; Write-Host ("  {0,-46} Acc@5 {1}/{2} ({3}%)  Acc@10 {4}/{2} ({5}%)  ids: {6}" -f $k,$r.h5,$r.n,(Pct $r.h5 $r.n),$r.h10,(Pct $r.h10 $r.n),(@($r.ids) -join ',')) }
if (-not $SkipDigest) { Write-Host ("  {0,-46} Acc@5 {1}/{2} ({3}%)  Acc@10 {4}/{2} ({5}%)  ids: {6}" -f 'B digest PT--MT-EN (sialia model)',$dig.h5,$dig.n,(Pct $dig.h5 $dig.n),$dig.h10,(Pct $dig.h10 $dig.n),(@($dig.ids) -join ',')) }
Write-Host "Wrote $OutPath"
