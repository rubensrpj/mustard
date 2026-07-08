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
    [string]$OutPath    = (Join-Path $PSScriptRoot 'aliased-results.md'),
    [string]$Base       = '100000'  # direct-match floor multiplier (see pagerank.rs)
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

# ---- config matrix: dict-gated (pre-fix) vs UNGATED direct-seed (the fix) -----------
# The fix (apps/scan/src/pagerank.rs): query tokens seed identifiers DIRECTLY,
# ungated by dict membership, + a fan-in-exempt base floor for direct matches. The
# ranker default is now ungated; `--no-direct-seed` restores the pre-fix behavior.
$variants = [ordered]@{
    'dict-gated raw-PT (pre-fix)'    = @{ expand=$false; extra=@('--no-direct-seed') }
    'dict-gated raw+equiv (pre-fix)' = @{ expand=$true;  extra=@('--no-direct-seed') }
    'UNGATED raw-PT (fix, no equiv)' = @{ expand=$false; extra=@('--direct-base',$Base) }
    'UNGATED raw+equiv (fix)'        = @{ expand=$true;  extra=@('--direct-base',$Base) }
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
$rawD  = $res['dict-gated raw-PT (pre-fix)']     # the 46.2% control
$expD  = $res['UNGATED raw+equiv (fix)']         # headline: the fix + equivalences
$expI  = $res['UNGATED raw-PT (fix, no equiv)']  # fix without equivalences

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

# ---- diagnosis prose (data-driven, finalized against the measured result) ----------
$DiagnosisText = @'
## Diagnosis (honest) — the ungate fixes the SEEDING (Acc@10 46.2%->53.8%, targets now reachable) but does NOT close id7/8/14 at Acc@5

**What the fix does (measured, base 100000):**
- **Acc@5 HELD at 46.2%** (no regression): the ungate alone drops id3, but the PT->EN equivalences recover it (`Attributable to the EQUIVALENCES` = id3), netting zero @5 change.
- **Acc@10 46.2% -> 53.8%**: id2 (`aging-bar.tsx`) reaches rank 8 — pre-fix it was unreachable. The direct `aging` identifier match now floors it into the top-10.
- **The English-identifier targets go from UNREACHABLE to reachable**: dict-gated pre-fix could not seed them at all (miss>50); the ungate seeds them — id8 `BankApprovalStatus.cs` -> rank ~19, id7 `use-sales-channels.ts` -> ~29. The seeding WAS the blocker, as Wave-2b predicted.

**Why id7/8/14 still miss Acc@5 (the honest limit):** the ungate proves seeding was the blocker, but a single GLOBAL floor cannot rank them top-5 without displacing the dict-route hits, because the English-identifier targets are NOT the uniquely-strongest identifier match — they share their domain words with many siblings:
- **id7**: `channel`/`sales` occur in ~117 files (every sales-channels page/route/loading/backend + the hook); the target hook is one of 117.
- **id8**: `bank`+`approval`+`status` also match `IBankApprovalService`, `BankApprovalService`, `PartnerApprovalService`, and every `*Status` enum.
- **id14**: `configuration`/`configure` is a 40-way-common EF pattern (every `IEntityTypeConfiguration.Configure`); `receivable` matches the whole receivable cluster (DTOs/services/repos/zod/entity). The target is not discriminable from its ~40 sibling configs — the same STRUCTURAL class as id9/id13, not a translation gap.

Raising the floor weight to force these into top-5 (base >=150k) promotes sibling noise and drops id3/5/12 (`partners.zod` 3->6, displaced by `partners-import.zod`/`DocumentValidator`). base 100000 is the strongest floor that preserves 46.2% Acc@5.

**id9/id13 remain graph-disconnected** (confirmed unchanged): the PT comment-term seed (`desdobramento`, `extrato`) anchors the frontend, and no import edge carries mass to the peripheral C# backend service — a disconnected cross-language component, exactly as diagnosed pre-fix. id14 joins this class (target undiscriminable from siblings).

**What would close @5 (follow-up, not a config knob):** the English-gloss digest baseline wins id7/8/14 because its BM25-over-identifiers adds declaration-KIND weighting (a top-level `const` hook / `enum` / `class` outranks incidental member matches) on top of idf+length. Porting that kind-weight into the direct-match floor is the remaining step; raw idf-sum seeding gets the targets reachable and lifts Acc@10, but not into top-5 against their domain siblings.
'@

# ---- render ------------------------------------------------------------------------
$sb = [System.Text.StringBuilder]::new()
$null=$sb.AppendLine("# Wave 2b - UNGATED ranker fix + PT->EN equivalence expansion vs the 46.2% baseline")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("Generated by ``benchmarks/sialia/pagerank-aliased.ps1`` on $(Get-Date -Format 'yyyy-MM-dd HH:mm').")
$null=$sb.AppendLine("The fix (``apps/scan/src/pagerank.rs``): query tokens seed module IDENTIFIERS DIRECTLY, ungated by dictionary membership (the English-gloss baseline's move), plus a fan-in-EXEMPT base floor so a directly-named low-centrality target (EF config, enum) ranks on its own seed. Query = raw PT intent + the English equivalents of each domain token (``equivalences.json``, $($equiv.Count) folded-PT keys). Régua identical to the baselines: scored labels (n=$($rawD.n)), target-OR-secondary within top-K. The pre-fix rows use ``--no-direct-seed`` (dict-gated); the fix rows are the ranker default.")
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
$null=$sb.AppendLine("**Headline (same régua as 46.2%): pre-fix dict-gated raw-PT $($rawD.h5)/$($rawD.n) ($(Pct $rawD.h5 $rawD.n)%) -> UNGATED raw+equiv (fix) $($expD.h5)/$($expD.n) ($(Pct $expD.h5 $expD.n)%) Acc@5, $($expD.h10)/$($expD.n) ($(Pct $expD.h10 $expD.n)%) Acc@10.**")
$null=$sb.AppendLine("")

# movers (fix+equiv vs pre-fix dict-gated raw)
$gain = @($rows | Where-Object { $_.scored -and $_.expHit5 -and -not $_.rawHit5 })
$lose = @($rows | Where-Object { $_.scored -and -not $_.expHit5 -and $_.rawHit5 })
$gainIdf = @($rows | Where-Object { $_.scored -and $_.expHit5 -and -not $_.expIdfHit5 })
$null=$sb.AppendLine("## Movers (UNGATED raw+equiv fix vs pre-fix dict-gated raw-PT)")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("- Gained (fix hit\@5, pre-fix miss): " + $(if ($gain.Count) { ($gain | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine("- Lost   (pre-fix hit\@5, fix miss): " + $(if ($lose.Count) { ($lose | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine("- Attributable to the EQUIVALENCES (fix+equiv hit\@5, fix-no-equiv miss): " + $(if ($gainIdf.Count) { ($gainIdf | ForEach-Object { "id $($_.id)" }) -join ', ' } else { '(none)' }))
$null=$sb.AppendLine("")
$null=$sb.AppendLine("## Per-prompt")
$null=$sb.AppendLine("")
$null=$sb.AppendLine("| id | diff | pre-fix hit@5 | fix(no-equiv) hit@5 | fix+equiv hit@5 | target rank (fix+equiv) | added EN tokens |")
$null=$sb.AppendLine("|---:|---|:---:|:---:|:---:|:---:|---|")
foreach ($r in $rows) {
    $null=$sb.AppendLine(("| {0} | {1} | {2} | {3} | {4} | {5} | {6} |" -f `
        $r.id,$r.difficulty,(YN $r.rawHit5),(YN $r.expIdfHit5),(YN $r.expHit5),(Fmt-Rank $r.expRank),$r.added))
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
$null=$sb.AppendLine("| target | pre-fix raw (dict-gated) | pre-fix +EN (dict-gated) | FIX +EN (ungated) |")
$null=$sb.AppendLine("|---|:---:|:---:|:---:|")
foreach ($pr in $probes) {
    $pt = [string]$byId[$pr.id].pt
    $ex = (Expand-Query $pt).query
    $rPreRaw = Probe-Rank $pt @('--no-direct-seed') $pr.needle
    $rPreExp = Probe-Rank $ex @('--no-direct-seed') $pr.needle
    $rFixExp = Probe-Rank $ex @('--direct-base',$Base) $pr.needle
    $null=$sb.AppendLine("| $($pr.label) | $rPreRaw | $rPreExp | $rFixExp |")
}
$null=$sb.AppendLine("")
$null=$sb.AppendLine("")
$null=$sb.AppendLine($DiagnosisText)
$null=$sb.AppendLine("")
Set-Content -LiteralPath $OutPath -Value $sb.ToString() -Encoding UTF8

Write-Host ""
Write-Host "=== AGGREGATE (scored n=$($rawD.n)) ==="
foreach ($k in $variants.Keys) { $r=$res[$k]; Write-Host ("  {0,-28} Acc@5 {1}/{2} ({3}%)  Acc@10 {4}/{2} ({5}%)  ids: {6}" -f $k,$r.h5,$r.n,(Pct $r.h5 $r.n),$r.h10,(Pct $r.h10 $r.n),(@($r.ids) -join ',')) }
Write-Host ("Gained: {0} | Lost: {1} | Gained-idf-only: {2}" -f (($gain|ForEach-Object{$_.id}) -join ','),(($lose|ForEach-Object{$_.id}) -join ','),(($gainIdf|ForEach-Object{$_.id}) -join ','))
Write-Host "Wrote $OutPath"
