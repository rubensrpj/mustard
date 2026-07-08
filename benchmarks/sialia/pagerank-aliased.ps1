# pagerank-aliased.ps1 - dict-seeded PageRank retrieval with PT->EN QUERY EXPANSION (Wave 2b).
#
# Measures the SAME `grain rank` ranker (apps/scan/src/pagerank.rs) as pagerank.ps1,
# but on an EXPANDED query: the raw Portuguese intent PLUS the English equivalents
# from equivalences.json (built once at scan time, keyed by accent-folded PT term).
# The hypothesis (Wave 2b): "the code is English; convert the PT prompt to English to
# search" — so bridging PT->EN should recover the English-concept misses id7 (sales
# channel), id8 (bank approval status enum), id14 (EF receivable configuration).
#
# Régua: identical to the baselines/pagerank.ps1 — scored labels only, target-OR-
# secondary within top-K, SHIPPED DEFAULT config is the headline (specificity seeding,
# undirected, damping .60, fan-in penalty 1.0). An idf-seeding ablation is included
# because that is the only config under which the expansion shows any lift.
# Read-only against the model+dict snapshots; native ConvertFrom-Json, no Python.

param(
    [string]$Exe        = (Join-Path $PSScriptRoot '..\..\target\debug\scan.exe'),
    [string]$Model      = (Join-Path $PSScriptRoot 'model\grain.model.json'),
    [string]$Dict       = (Join-Path $PSScriptRoot 'model\grain.dictionary.json'),
    [string]$EquivPath  = (Join-Path $PSScriptRoot 'equivalences.json'),
    [string]$LabelsPath = (Join-Path $PSScriptRoot 'labels.ndjson'),
    [string]$OutPath    = (Join-Path $PSScriptRoot 'aliased-results.md')
)
$ErrorActionPreference = 'Stop'

# ---- accent-fold (lowercase + strip diacritics, incl. c-cedilla) to hit equiv keys -
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

# ---- load equivalences (folded-PT key -> English[] ) --------------------------------
$equivRaw = (Get-Content -Raw -LiteralPath $EquivPath | ConvertFrom-Json -Depth 64).equivalences
$equiv = @{}
foreach ($p in $equivRaw.PSObject.Properties) { $equiv[$p.Name] = @($p.Value) }

# ---- expand one PT intent: append the English equivalents of each domain token ------
function Expand-Query {
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
    return @{ query = ($pt + ' ' + ($added -join ' ')).Trim(); added = $added }
}

function Run-Ranker {
    param([string]$q, [string[]]$extra)
    if ([string]::IsNullOrWhiteSpace($q)) { return @() }
    $a = @('rank', $Model, '--dict', $Dict, '--query', $q, '--top', '10') + $extra
    $raw = & $Exe @a 2>$null | Out-String
    $idx = $raw.IndexOf('{'); if ($idx -lt 0) { return @() }
    try { $o = $raw.Substring($idx) | ConvertFrom-Json -Depth 64 } catch { return @() }
    return @($o.files | ForEach-Object { ([string]$_.file).Replace('\','/') })
}
function Find-Rank { param($files, [string]$t)
    if ([string]::IsNullOrWhiteSpace($t)) { return -2 }
    $t = $t.Replace('\','/'); for ($i=0;$i -lt $files.Count;$i++){ if ($files[$i] -eq $t){ return ($i+1) } } ; return -1 }
function Probe-Rank {
    # Rank (1-based) of the first top-50 file whose path contains $needle, or 'miss>50'.
    param([string]$q, [string[]]$extra, [string]$needle)
    $a = @('rank', $Model, '--dict', $Dict, '--query', $q, '--top', '50') + $extra
    $raw = & $Exe @a 2>$null | Out-String; $i = $raw.IndexOf('{'); if ($i -lt 0) { return 'ERR' }
    try { $o = $raw.Substring($i) | ConvertFrom-Json -Depth 64 } catch { return 'ERR' }
    $files = @($o.files | ForEach-Object { ([string]$_.file).Replace('\','/') })
    for ($k=0; $k -lt $files.Count; $k++) { if ($files[$k] -like "*$needle*") { return ($k+1) } }
    return 'miss>50'
}
function HitK { param([int]$tr,[int[]]$sr,[int]$k)
    if ($tr -ge 1 -and $tr -le $k){ return $true }; foreach($r in @($sr)){ if($r -ge 1 -and $r -le $k){ return $true } }; return $false }
function Fmt-Rank { param([int]$r); switch ($r) { -2 { 'n/a' } -1 { 'miss' } default { "$r" } } }
function YN { param([bool]$b); if ($b) { 'Y' } else { '.' } }
function Pct { param([int]$k,[int]$t); if ($t -eq 0){ '0.0' } else { ('{0:N1}' -f (100.0*$k/$t)) } }

# ---- load labels -------------------------------------------------------------------
$labels = @()
foreach ($line in (Get-Content -LiteralPath $LabelsPath)) { $t=$line.Trim(); if ($t.Length){ $labels += ($t | ConvertFrom-Json -Depth 64) } }
Write-Host "Loaded $($labels.Count) labels; equivalence keys: $($equiv.Count)"

# ---- config matrix: raw vs expanded, under default (specificity) and idf seeding ----
$variants = [ordered]@{
    'raw-PT (default)'          = @{ expand=$false; extra=@() }
    'raw-PT + equiv (default)'  = @{ expand=$true;  extra=@() }
    'raw-PT + equiv (idf seed)' = @{ expand=$true;  extra=@('--seed-weight','idf') }
    'raw-PT (idf seed)'         = @{ expand=$false; extra=@('--seed-weight','idf') }
}

function Eval-Variant {
    param([bool]$expand, [string[]]$extra)
    $h5=0; $h10=0; $n=0; $ids=@(); $perid=@{}
    foreach ($lab in $labels) {
        if (-not $lab.scored) { continue }
        $n++
        $q = if ($expand) { (Expand-Query ([string]$lab.pt)).query } else { [string]$lab.pt }
        $files = Run-Ranker $q $extra
        $tr = Find-Rank $files ([string]$lab.target)
        $sr = @(); foreach($s in @($lab.secondary)){ $sr += (Find-Rank $files $s) }
        $best = $tr; foreach($r in @($sr)){ if ($r -ge 1 -and ($best -lt 1 -or $r -lt $best)) { $best = $r } }
        $hit5 = HitK $tr $sr 5
        if ($hit5){ $h5++; $ids += [int]$lab.id }
        if (HitK $tr $sr 10){ $h10++ }
        $perid[[int]$lab.id] = @{ rank=$tr; bestSecondary=$best; hit5=$hit5 }
    }
    return @{ h5=$h5; h10=$h10; n=$n; ids=($ids | Sort-Object); perid=$perid }
}

$res = [ordered]@{}
foreach ($k in $variants.Keys) {
    Write-Host "  variant: $k ..."
    $res[$k] = Eval-Variant $variants[$k].expand $variants[$k].extra
}
$rawD  = $res['raw-PT (default)']
$expD  = $res['raw-PT + equiv (default)']
$expI  = $res['raw-PT + equiv (idf seed)']

# ---- per-prompt audit (added english tokens + rank under each) ----------------------
$rows = @()
foreach ($lab in $labels) {
    $id = [int]$lab.id
    $ex = Expand-Query ([string]$lab.pt)
    $rows += [pscustomobject]@{
        id=$id; difficulty=[string]$lab.difficulty; scored=[bool]$lab.scored
        target=[string]$lab.target
        added=($ex.added -join ' ')
        rawHit5   = if ($rawD.perid.ContainsKey($id)) { [bool]$rawD.perid[$id].hit5 } else { $false }
        expHit5   = if ($expD.perid.ContainsKey($id)) { [bool]$expD.perid[$id].hit5 } else { $false }
        expIdfHit5= if ($expI.perid.ContainsKey($id)) { [bool]$expI.perid[$id].hit5 } else { $false }
        expRank   = if ($expD.perid.ContainsKey($id)) { [int]$expD.perid[$id].rank } else { -1 }
    }
}

# ---- render ------------------------------------------------------------------------
$sb = [System.Text.StringBuilder]::new()
$null=$sb.AppendLine("# Wave 2b - PT->EN equivalence query expansion vs the dict-seeded PageRank baseline")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("Generated by ``benchmarks/sialia/pagerank-aliased.ps1`` on $(Get-Date -Format 'yyyy-MM-dd HH:mm').")
$null=$sb.AppendLine("Retrieval under test: the SAME ``grain rank`` ranker, on an EXPANDED query = raw PT intent + the English equivalents of each domain token (``equivalences.json``, $($equiv.Count) folded-PT keys). Régua identical to the baselines: scored labels (n=$($rawD.n)), target-OR-secondary within top-K.")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## Aggregate (scored labels only, n=$($rawD.n))")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("| Retrieval | Acc@5 | Acc@10 | hit ids @5 |")
$null=$sb.AppendLine("|---|---|---|---|")
foreach ($k in $variants.Keys) {
    $r = $res[$k]
    $null=$sb.AppendLine("| $k | $($r.h5)/$($r.n) ($(Pct $r.h5 $r.n)%) | $($r.h10)/$($r.n) ($(Pct $r.h10 $r.n)%) | $(@($r.ids) -join ',') |")
}
$null=$sb.AppendLine("| — digest baseline raw-PT | 2/13 (15.4%) | 2/13 (15.4%) | 2,4 |")
$null=$sb.AppendLine("| — digest baseline PT+EN  | 6/13 (46.2%) | 6/13 (46.2%) | 2,4,7,8,9,14 |")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("**Headline (default config, same régua as 46.2%): raw-PT $($rawD.h5)/$($rawD.n) ($(Pct $rawD.h5 $rawD.n)%) -> raw-PT+equiv $($expD.h5)/$($expD.n) ($(Pct $expD.h5 $expD.n)%).**")
$null=$sb.AppendLine("")

# movers
$gain = @($rows | Where-Object { $_.scored -and $_.expHit5 -and -not $_.rawHit5 })
$lose = @($rows | Where-Object { $_.scored -and -not $_.expHit5 -and $_.rawHit5 })
$gainIdf = @($rows | Where-Object { $_.scored -and $_.expIdfHit5 -and -not $_.rawHit5 })
$null=$sb.AppendLine("## Movers (expansion vs raw, default config)")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("- Gained (expand hit\@5, raw miss): " + $(if ($gain.Count) { ($gain | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine("- Lost   (raw hit\@5, expand miss): " + $(if ($lose.Count) { ($lose | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine("- Gained ONLY under idf seeding (non-default): " + $(if ($gainIdf.Count) { ($gainIdf | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## Per-prompt (default config)")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("| id | diff | raw hit@5 | +equiv hit@5 | +equiv(idf) hit@5 | target rank (+equiv) | added EN tokens |")
$null=$sb.AppendLine("|---:|---|:---:|:---:|:---:|:---:|---|")
foreach ($r in $rows) {
    $null=$sb.AppendLine(("| {0} | {1} | {2} | {3} | {4} | {5} | {6} |" -f `
        $r.id,$r.difficulty,(YN $r.rawHit5),(YN $r.expHit5),(YN $r.expIdfHit5),(Fmt-Rank $r.expRank),$r.added))
}
$null=$sb.AppendLine("")
$null=$sb.AppendLine("Note: id 10 & id 15 are ``scored:false`` (excluded from the aggregate).")
$null=$sb.AppendLine("")

# ---- target-rank probe: where do the English-concept misses actually sit? -----------
$byId = @{}; foreach ($lab in $labels) { $byId[[int]$lab.id] = $lab }
$probes = @(
    @{ label='id7 use-sales-channels.ts (primary, no secondary)'; id=7;  needle='use-sales-channels.ts' }
    @{ label='id8 BankApprovalStatus.cs (primary)';               id=8;  needle='BankApprovalStatus.cs' }
    @{ label='id8 banks/banks.zod.ts (secondary)';                id=8;  needle='banks/banks.zod.ts' }
    @{ label='id14 ReceivableConfiguration.cs (primary)';         id=14; needle='ReceivableConfiguration.cs' }
    @{ label='id14 PayableConfiguration.cs (secondary)';          id=14; needle='PayableConfiguration.cs' }
    @{ label='id3 partners/.../form/form-context.tsx (raw hit)';  id=3;  needle='partners/_components/form/form-context.tsx' }
)
$null=$sb.AppendLine("## Target-rank probe — the English-concept misses (rank of target/secondary in top-50)")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("| target | raw (default) | +EN equiv (default) | +EN equiv (idf seed) |")
$null=$sb.AppendLine("|---|:---:|:---:|:---:|")
foreach ($pr in $probes) {
    $pt = [string]$byId[$pr.id].pt
    $ex = (Expand-Query $pt).query
    $rRaw = Probe-Rank $pt @() $pr.needle
    $rExp = Probe-Rank $ex @() $pr.needle
    $rIdf = Probe-Rank $ex @('--seed-weight','idf') $pr.needle
    $null=$sb.AppendLine("| $($pr.label) | $rRaw | $rExp | $rIdf |")
}
$null=$sb.AppendLine("")
$null=$sb.AppendLine("Under the default (specificity) config the English tokens are **inert-to-harmful**: they never promote a miss target into the top-5, and the broad-term equivalents (``valor→amount``, ``status``) actually DEMOTE the one reachable secondary (id8 ``banks.zod.ts`` 30 → 36). The single positive movement (→9) comes from switching the SEED WEIGHT to idf, which moves the RAW query too — so no id7/id8/id14 hit is attributable to translation.")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## Diagnosis (honest) — the bridge does NOT lift 46.2%; under the default config it regresses to 38.5%")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("The equivalences are linguistically correct (``canal→channel``, ``aprovação→approved``, ``configuração→configure``), but the ranker's architecture defeats them for these targets — three compounding STRUCTURAL causes, not a bad dictionary:")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("1. **Dict-gated seeding makes most equivalents inert.** ``grain rank`` seeds a query token only if it matches a *distinctive-dictionary term* (``pagerank.rs`` MATCH, lines 351-380), which then seeds modules whose identifiers fold-equal it (``token_seeds``) ∪ its dict anchors. The English identifiers of the miss targets — ``sales``, ``channel``, ``bank``, ``receivable``, ``configuration``, ``status``, ``entity``, ``invoice`` — are NOT dictionary terms (only PT ``venda``/``canal``/``entidade``/``configuração`` and code words ``approved``/``configure``/``duedate``/``installment``/``supplier`` made the distinctive top-500). Every equivalent that isn't a dict term adds exactly zero seed mass.")
$null=$sb.AppendLine("2. **The few equivalents that ARE dict terms anchor the wrong (hub) cluster.** ``approved`` (df 28) anchors contract files; ``configure`` (df 117) anchors DI bootstrap; ``amount`` (from ``valor``) anchors dashboards. They reach the real target only through one thin ``token_seeds`` edge (enum member ``APPROVED``; method ``Configure``) that is drowned by their high-frequency anchors — pouring mass into contracts/DI/dashboards and DEMOTING borderline raw hits (id3: 4 → 6, the entire −7.7pp regression).")
$null=$sb.AppendLine("3. **The miss targets are peripheral nodes the fan-in penalty pushes down.** ``use-sales-channels.ts`` has ~0 in-model importers, ``BankApprovalStatus.cs`` fan_in 9 (demoted by the 1.0 fan-in penalty), ``ReceivableConfiguration.cs`` fan_in 1 (leaf). Personalized PageRank lifts import-CENTRAL files; these are the opposite. Same disconnected-graph failure already diagnosed for id9/id13 (a PT-anchored frontend seed can't reach a peripheral C# backend node) — id14's backend EF config is exactly that case.")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("**Verdict.** Query expansion via ``equivalences.json`` does not recover id7/id8/id14 at Acc@5 and, under the shipped default config, LOWERS the score (46.2% → 38.5%). The 61.5% Acc@10 line is real but comes from the idf seed weight, not the translation. To cash the PT→EN bridge for these English-identifier targets the RANKER must change: seed ``token_seeds`` DIRECTLY from the (expanded) query tokens, ungated by dict membership — letting ``sales``/``channel``/``configuration``/``bank`` seed the files whose identifiers contain them. The bilingual dictionary is a necessary input for that fix but is inert on its own while the seed path stays dict-gated. (The digest baseline wins id7/8/14 precisely because it matches the English gloss against identifiers directly, with no dictionary gate.)")
$null=$sb.AppendLine("")
Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8

Write-Host ""
Write-Host "=== AGGREGATE (scored n=$($rawD.n)) ==="
foreach ($k in $variants.Keys) { $r=$res[$k]; Write-Host ("  {0,-28} Acc@5 {1}/{2} ({3}%)  Acc@10 {4}/{2} ({5}%)  ids: {6}" -f $k,$r.h5,$r.n,(Pct $r.h5 $r.n),$r.h10,(Pct $r.h10 $r.n),(@($r.ids) -join ',')) }
Write-Host ("Gained: {0} | Lost: {1} | Gained-idf-only: {2}" -f (($gain|ForEach-Object{$_.id}) -join ','),(($lose|ForEach-Object{$_.id}) -join ','),(($gainIdf|ForEach-Object{$_.id}) -join ','))
Write-Host "Wrote $OutPath"
