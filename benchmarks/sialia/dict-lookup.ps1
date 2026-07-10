# dict-lookup.ps1 - dictionary-ONLY retrieval vs sialia labels (Wave 2a validation).
#
# Measures the retrieval power of the `grain.dictionary.json` sidecar ALONE - no
# digest, no graph, no LLM - against the labeled prompt set, using ONLY the raw
# Portuguese intent (`<pt>`). The question it answers: does the PT domain
# vocabulary mined from COMMENTS bridge a raw-PT query straight to the (mostly
# English/mixed) target files, WITHOUT the English gloss the JUSTO baseline needs?
#
# Compared against the already-measured digest baseline (benchmarks/sialia/
# baseline-results.md): PT raw = 15.4% (2/13), PT+EN = 46.2% (6/13) Acc@5.
#
# Algorithm (per label, `<pt>` only):
#   1. Tokenize <pt>: split on non-alphanumeric, lowercase, >=3 chars, drop PT+EN
#      common stopwords (checked accent-folded).
#   2. Match each token to dict `term`s: folded-equality (EXACT or accent-fold)
#      OR folded-prefix (either direction, shorter side >=4 chars).
#   3. Union the `anchors` of the matched (deduped) terms; each anchor file's
#      score = sum, over matched terms that list it, of `specificity_x1024`.
#   4. Rank files by score DESC (path ASC tiebreak) -> top-N.
#   5. hit@5 / hit@10 = target OR any secondary within top-K; record target rank.
#   Aggregate Acc@5 / Acc@10 over `scored:true` labels only.
#
# The dictionary is a pre-built read-only snapshot (scan of C:\Atiz\sialia); this
# script never touches sialia. ConvertFrom-Json native, no Python.

param(
    [string]$DictPath     = (Join-Path $PSScriptRoot 'grain.dictionary.json'),
    [string]$LabelsPath   = (Join-Path $PSScriptRoot 'labels.ndjson'),
    [string]$BaselinePath = (Join-Path $PSScriptRoot 'baseline-raw.json'),
    [string]$OutPath      = (Join-Path $PSScriptRoot 'dict-lookup-results.md'),
    [string]$RawPath      = (Join-Path $PSScriptRoot 'dict-lookup-raw.json')
)

$ErrorActionPreference = 'Stop'

# ---- stopwords (PT + EN COMMON function words; never domain terms) ----------
# Articles, prepositions, conjunctions, pronouns, common auxiliaries and
# interrogatives - close to the NLTK PT list plus a standard EN list. Stored
# accent-folded; the token is folded before the membership test.
$STOPWORDS = @(
    # PT function words
    'de','do','da','dos','das','no','na','nos','nas','em','num','numa','ao','aos',
    'os','as','um','uma','uns','umas','que','se','por','para','com','sem','sob',
    'sobre','entre','ate','apos','ante','pelo','pela','pelos','pelas','este','esta',
    'esse','essa','isso','isto','aquele','aquela','seu','sua','seus','suas','meu',
    'minha','nao','sim','mais','menos','muito','pouco','ser','sao','foi','era','esta',
    'estao','ter','tem','como','quando','qual','quais','quem','onde','tambem','ja',
    'entao','cada','todo','toda','todos','todas',
    # EN function words
    'the','and','not','are','was','for','with','into','from','has','have','that',
    'this','out','its','can','will','add','new'
)
$STOPSET = [System.Collections.Generic.HashSet[string]]::new()
foreach ($w in $STOPWORDS) { [void]$STOPSET.Add($w) }

# ---- helpers ---------------------------------------------------------------

function Fold {
    # Accent-fold: strip combining marks (NFD, drop NonSpacingMark), lowercase.
    param([string]$s)
    if ([string]::IsNullOrEmpty($s)) { return '' }
    $d = $s.Normalize([System.Text.NormalizationForm]::FormD)
    $sb = [System.Text.StringBuilder]::new()
    foreach ($ch in $d.ToCharArray()) {
        if ([System.Globalization.CharUnicodeInfo]::GetUnicodeCategory($ch) -ne [System.Globalization.UnicodeCategory]::NonSpacingMark) {
            [void]$sb.Append($ch)
        }
    }
    $sb.ToString().Normalize([System.Text.NormalizationForm]::FormC).ToLowerInvariant()
}

function Get-Tokens {
    # Split on non-alphanumeric, lowercase, floor 3 chars, drop stopwords (folded),
    # require at least one letter. Returns DEDUPED folded tokens.
    param([string]$text)
    $out  = [System.Collections.Generic.List[string]]::new()
    $seen = [System.Collections.Generic.HashSet[string]]::new()
    if ([string]::IsNullOrWhiteSpace($text)) { return ,$out.ToArray() }
    foreach ($p in [System.Text.RegularExpressions.Regex]::Split($text, '[^\p{L}\p{Nd}]+')) {
        if ($p.Length -lt 3) { continue }
        $f = Fold $p
        if ($f.Length -lt 3) { continue }
        if (-not ($f -match '[a-z]')) { continue }   # drop pure-digit tokens
        if ($STOPSET.Contains($f)) { continue }
        if ($seen.Add($f)) { [void]$out.Add($f) }
    }
    ,$out.ToArray()
}

function Test-HitAtK {
    param([int]$TargetRank, [int[]]$SecRanks, [int]$K)
    if ($TargetRank -ge 1 -and $TargetRank -le $K) { return $true }
    foreach ($r in @($SecRanks)) { if ($r -ge 1 -and $r -le $K) { return $true } }
    return $false
}

function Find-Rank {
    # -2 = no target (n/a); -1 = target defined but absent; else 1-based rank.
    param($rankedFiles, [string]$target)
    if ([string]::IsNullOrWhiteSpace($target)) { return -2 }
    $t = $target.Replace('\','/')
    for ($i = 0; $i -lt $rankedFiles.Count; $i++) {
        if ($rankedFiles[$i].file -eq $t) { return ($i + 1) }
    }
    return -1
}

function Fmt-Rank { param([int]$r); switch ($r) { -2 { 'n/a' } -1 { 'miss' } default { "$r" } } }
function YN { param([bool]$b); if ($b) { 'Y' } else { '.' } }
function Pct { param([int]$k, [int]$tot); if ($tot -eq 0) { '0.0' } else { ('{0:N1}' -f (100.0 * $k / $tot)) } }

# ---- load dictionary (pre-fold every term once) ----------------------------

$dict  = Get-Content -LiteralPath $DictPath -Raw | ConvertFrom-Json -Depth 64
$terms = @($dict.terms)
$N     = $terms.Count
$fterm = New-Object 'string[]' $N
for ($i = 0; $i -lt $N; $i++) { $fterm[$i] = Fold ([string]$terms[$i].term) }
Write-Host "Loaded dictionary v$($dict.version): $N terms from $DictPath"

function Match-TermIdx {
    # For a set of folded tokens, return the deduped set of matching term indices.
    param([string[]]$ftokens)
    $idx = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($tok in $ftokens) {
        $tl = $tok.Length
        for ($i = 0; $i -lt $N; $i++) {
            $fm = $fterm[$i]
            if ($tok -eq $fm) { [void]$idx.Add($i); continue }
            $ml = $fm.Length
            $minl = if ($tl -lt $ml) { $tl } else { $ml }
            if ($minl -ge 4 -and ($fm.StartsWith($tok, [System.StringComparison]::Ordinal) -or $tok.StartsWith($fm, [System.StringComparison]::Ordinal))) {
                [void]$idx.Add($i)
            }
        }
    }
    $idx
}

function Rank-Files {
    # Union anchors of matched terms; file score = sum of specificity_x1024 over
    # matched terms listing it. Sorted score DESC, path ASC.
    param([int[]]$termIdx)
    $score = @{}
    foreach ($i in $termIdx) {
        $t = $terms[$i]
        $s = [long]$t.specificity_x1024
        foreach ($a in @($t.anchors)) {
            $f = ([string]$a).Replace('\','/')
            if ($score.ContainsKey($f)) { $score[$f] += $s } else { $score[$f] = $s }
        }
    }
    ,@($score.GetEnumerator() |
        ForEach-Object { [pscustomobject]@{ file = $_.Key; score = [long]$_.Value } } |
        Sort-Object -Property @{Expression='score';Descending=$true}, @{Expression='file';Descending=$false})
}

# ---- load labels + baseline (join by id) -----------------------------------

$labels = @()
foreach ($line in (Get-Content -LiteralPath $LabelsPath)) {
    $t = $line.Trim(); if ($t.Length -eq 0) { continue }
    $labels += ($t | ConvertFrom-Json -Depth 64)
}

$base = @{}
if (Test-Path -LiteralPath $BaselinePath) {
    foreach ($b in @(Get-Content -LiteralPath $BaselinePath -Raw | ConvertFrom-Json -Depth 64)) {
        $base[[int]$b.id] = $b
    }
}
Write-Host "Loaded $($labels.Count) labels; baseline rows: $($base.Count)"

# ---- run -------------------------------------------------------------------

$results = @()
foreach ($lab in $labels) {
    $tokens = Get-Tokens ([string]$lab.pt)
    $idxSet = Match-TermIdx $tokens
    $idxArr = @($idxSet)
    $ranked = Rank-Files $idxArr

    $sec = @($lab.secondary)
    $tRank = Find-Rank $ranked $lab.target
    $secRanks = @(); foreach ($s in $sec) { $secRanks += (Find-Rank $ranked $s) }

    # Matched terms, most-specific first (for the leakage / bridging analysis).
    $matched = @(
        $idxArr | ForEach-Object { $terms[$_] } |
            Sort-Object -Property @{Expression='specificity_x1024';Descending=$true}, @{Expression='term';Descending=$false} |
            ForEach-Object { [pscustomobject]@{ term = [string]$_.term; spec = [long]$_.specificity_x1024; source = [string]$_.source } }
    )

    $b = $base[[int]$lab.id]
    $results += [pscustomobject]@{
        id           = [int]$lab.id
        difficulty   = [string]$lab.difficulty
        scored       = [bool]$lab.scored
        target       = [string]$lab.target
        secondary    = $sec
        note         = [string]$lab.note
        tokens       = $tokens
        matchedCount = $matched.Count
        matched      = @($matched | Select-Object -First 12)
        targetRank   = $tRank
        secRanks     = $secRanks
        hit5         = (Test-HitAtK $tRank $secRanks 5)
        hit10        = (Test-HitAtK $tRank $secRanks 10)
        top          = @($ranked | Select-Object -First 6)
        fileCount    = $ranked.Count
        # baseline (digest) for the same id
        cruHit5      = if ($b) { [bool]$b.cruHit5 }   else { $false }
        justoHit5    = if ($b) { [bool]$b.justoHit5 } else { $false }
        cruRank      = if ($b) { [int]$b.cruTargetRank }   else { 0 }
        justoRank    = if ($b) { [int]$b.justoTargetRank } else { 0 }
    }
}

# ---- aggregate (scored:true only) ------------------------------------------

$scored = @($results | Where-Object { $_.scored })
$n = $scored.Count
$dHit5  = @($scored | Where-Object { $_.hit5  }).Count
$dHit10 = @($scored | Where-Object { $_.hit10 }).Count
$cruHit5   = @($scored | Where-Object { $_.cruHit5   }).Count
$justoHit5 = @($scored | Where-Object { $_.justoHit5 }).Count

# ---- render markdown -------------------------------------------------------

$sb = [System.Text.StringBuilder]::new()
$null = $sb.AppendLine("# Dictionary-only retrieval vs sialia labels (Wave 2a validation)")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("Generated by ``benchmarks/sialia/dict-lookup.ps1`` on $(Get-Date -Format 'yyyy-MM-dd HH:mm').")
$null = $sb.AppendLine("Retrieval under test: the ``grain.dictionary.json`` sidecar ALONE (scan of ``C:\Atiz\sialia``), queried with the RAW Portuguese intent only - no digest, no graph, no English gloss.")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("- Match: token vs ``term`` folded-equality (exact / accent-fold) OR folded-prefix (either direction, shorter side >=4 chars).")
$null = $sb.AppendLine("- File score = sum of ``specificity_x1024`` over matched terms whose ``anchors`` list it; ranked score DESC, path ASC.")
$null = $sb.AppendLine("- ``rank`` = 1-based position of the **target**; ``miss`` = target defined but absent; ``n/a`` = no target (id 10).")
$null = $sb.AppendLine("- ``hit@5`` = target OR any secondary within top-5.")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("## Aggregate (scored labels only, n=$n)")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("| Metric | DICT-only (PT raw) | baseline digest PT raw | baseline digest PT+EN |")
$null = $sb.AppendLine("|---|---|---|---|")
$null = $sb.AppendLine("| Acc@5  | $dHit5/$n ($(Pct $dHit5 $n)%) | $cruHit5/$n ($(Pct $cruHit5 $n)%) | $justoHit5/$n ($(Pct $justoHit5 $n)%) |")
$null = $sb.AppendLine("| Acc@10 | $dHit10/$n ($(Pct $dHit10 $n)%) | 2/13 (15.4%) | 6/13 (46.2%) |")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("Baselines quoted from ``benchmarks/sialia/baseline-results.md`` (digest via ``mustard-rt run feature --intent``). The dict-only column uses the SAME raw-PT intent as the digest PT-raw column - a like-for-like isolation of what the mined vocabulary alone buys.")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("## Per-prompt")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("| id | diff | scored | #match | rank(target) DICT | hit@5 DICT | hit@10 DICT | hit@5 PTraw | hit@5 PT+EN |")
$null = $sb.AppendLine("|---:|---|:---:|---:|:---:|:---:|:---:|:---:|:---:|")
foreach ($r in $results) {
    $sc = if ($r.scored) { 'yes' } else { 'no' }
    $null = $sb.AppendLine(("| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7} | {8} |" -f `
        $r.id, $r.difficulty, $sc, $r.matchedCount, (Fmt-Rank $r.targetRank), `
        (YN $r.hit5), (YN $r.hit10), (YN $r.cruHit5), (YN $r.justoHit5)))
}
$null = $sb.AppendLine("")
$null = $sb.AppendLine("Note: id 10 & id 15 are ``scored:false`` (excluded from the aggregate).")
$null = $sb.AppendLine("")

# movers vs the two baselines (scored only)
$vsRaw   = @($scored | Where-Object { $_.hit5 -and -not $_.cruHit5 })
$loseRaw = @($scored | Where-Object { -not $_.hit5 -and $_.cruHit5 })
$vsJusto = @($scored | Where-Object { $_.hit5 -and -not $_.justoHit5 })
$loseJusto = @($scored | Where-Object { -not $_.hit5 -and $_.justoHit5 })
$null = $sb.AppendLine("## Movers")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("- DICT-only wins over digest PT-raw (dict hit\@5, raw miss): " + $(if ($vsRaw.Count) { ($vsRaw | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null = $sb.AppendLine("- DICT-only loses to digest PT-raw (raw hit\@5, dict miss): " + $(if ($loseRaw.Count) { ($loseRaw | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null = $sb.AppendLine("- DICT-only wins over digest PT+EN (dict hit\@5, PT+EN miss): " + $(if ($vsJusto.Count) { ($vsJusto | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null = $sb.AppendLine("- DICT-only loses to digest PT+EN (PT+EN hit\@5, dict miss): " + $(if ($loseJusto.Count) { ($loseJusto | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null = $sb.AppendLine("")

# diagnosis: split the two capabilities the dict-only path conflates -----------
$bridged  = @($scored | Where-Object { $_.matchedCount -ge 1 }).Count
$bridged5 = @($scored | Where-Object { $_.matchedCount -ge 5 }).Count
$avgMatch = if ($n) { '{0:N1}' -f ((@($scored | ForEach-Object { $_.matchedCount }) | Measure-Object -Sum).Sum / $n) } else { '0' }
# recurring top-5 files across scored prompts = the centrality hubs that capture
# the anchor slots (the plumbing leak).
$hubCount = @{}
foreach ($r in $scored) {
    foreach ($f in @($r.top | Select-Object -First 5)) {
        $k = [string]$f.file
        if ($hubCount.ContainsKey($k)) { $hubCount[$k]++ } else { $hubCount[$k] = 1 }
    }
}
$hubs = @($hubCount.GetEnumerator() | Where-Object { $_.Value -ge 2 } |
    Sort-Object -Property @{Expression='Value';Descending=$true}, @{Expression='Key';Descending=$false})

$null = $sb.AppendLine("## Diagnosis: vocabulary bridge fires, anchor localization does not")
$null = $sb.AppendLine("")
$null = $sb.AppendLine("- **The PT->term bridge works.** $bridged/$n scored prompts matched at least one distinctive PT domain term (mined from comments), $bridged5/$n matched >=5; mean $avgMatch matched terms/prompt. Raw PT queries land on ``contrato``, ``vencimento``, ``parceiro``, ``titulo``, ``aprovacao``, ``conciliacao`` etc. WITHOUT any English gloss - exactly the cross-lingual lift Wave 2a set out to prove.")
$null = $sb.AppendLine("- **The anchors cannot localize.** Only $dHit5/$n reach the target file, BELOW the digest's own raw-PT $cruHit5/$n. Each term's <=3 ``anchors`` are the files where it is most FREQUENT (most central), i.e. comment-dense hubs - not the specific file a task edits. The lone hit (id 12) is the case where the target IS that hub (``ContractService.Create.cs`` is the centrality anchor for contrato+criacao+validacao).")
$null = $sb.AppendLine("- **Plumbing/hub leak into the anchor slots** (files recurring across scored top-5s):")
foreach ($h in $hubs) { $null = $sb.AppendLine("  - $($h.Value)x  ``$($h.Key)``") }
$null = $sb.AppendLine("  These are GraphQL type-extension hubs, the ``ApiExceptionErrorCodes`` enum, ``AuthService``, ``base-entity-hooks`` - comment-dense files that win the term-frequency centrality race and so shadow the real target. The miner's ubiquity ceiling also let raw plumbing terms (``graphql``, ``inheritdoc``, ``null``, ``google``, ``backend``) into the top of the dictionary by specificity, consuming cap slots though they never match a PT domain query directly.")
$null = $sb.AppendLine("- **Verdict for Wave 3.** Keep the dictionary as the TRANSLATION/vocabulary layer (its term recall is strong); do NOT retrieve on its anchors. Fuse the matched terms into the digest's file-level scoring instead of trusting the dict's centrality anchors.")
$null = $sb.AppendLine("")

# per-prompt detail: matched terms + top-5 files (bridging + leakage evidence)
$null = $sb.AppendLine("## Detail (matched terms most-specific-first, and top-5 files)")
$null = $sb.AppendLine("")
foreach ($r in $results) {
    $null = $sb.AppendLine("### id $($r.id) [$($r.difficulty), scored=$($r.scored)] - rank(target) DICT = $(Fmt-Rank $r.targetRank), hit@5=$(YN $r.hit5)")
    $null = $sb.AppendLine("")
    $null = $sb.AppendLine("- target: ``$($r.target)``")
    if (@($r.secondary).Count) { $null = $sb.AppendLine("- secondary ranks: " + (@(0..(@($r.secondary).Count-1)) | ForEach-Object { "``$($r.secondary[$_])`` = $(Fmt-Rank $r.secRanks[$_])" }) -join '; ') }
    $mt = @($r.matched | ForEach-Object { "$($_.term)($($_.spec)/$($_.source))" }) -join ', '
    $null = $sb.AppendLine("- matched terms ($($r.matchedCount)): $mt")
    $null = $sb.AppendLine("- top-5 files:")
    if (@($r.top).Count -eq 0) { $null = $sb.AppendLine("  - (empty - no term matched)") }
    foreach ($f in @($r.top | Select-Object -First 5)) { $null = $sb.AppendLine("  - $($f.score)  ``$($f.file)``") }
    $null = $sb.AppendLine("")
}

Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8
$results | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $RawPath -Encoding UTF8

# ---- console summary -------------------------------------------------------
Write-Host ""
Write-Host "=== AGGREGATE (scored n=$n) ==="
Write-Host ("Acc@5   DICT-only(PTraw) {0}/{1} ({2}%)   |  digest PTraw {3}/{1} ({4}%)   |  digest PT+EN {5}/{1} ({6}%)" -f $dHit5, $n, (Pct $dHit5 $n), $cruHit5, (Pct $cruHit5 $n), $justoHit5, (Pct $justoHit5 $n))
Write-Host ("Acc@10  DICT-only(PTraw) {0}/{1} ({2}%)" -f $dHit10, $n, (Pct $dHit10 $n))
Write-Host ("DICT wins vs PTraw @5: " + $(if ($vsRaw.Count) { ($vsRaw | ForEach-Object { $_.id }) -join ',' } else { '-' }))
Write-Host ("DICT wins vs PT+EN @5: " + $(if ($vsJusto.Count) { ($vsJusto | ForEach-Object { $_.id }) -join ',' } else { '-' }))
Write-Host ("DICT loses vs PT+EN @5: " + $(if ($loseJusto.Count) { ($loseJusto | ForEach-Object { $_.id }) -join ',' } else { '-' }))
Write-Host "Wrote $OutPath"
Write-Host "Wrote $RawPath"
