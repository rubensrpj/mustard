# install-retrieval.ps1 — instala a solução de retrieval e liga no sialia (1 comando).
# O que faz: (1) instala mustard-rt + scan + mustard-translate em ~/.cargo/bin
# (com backup .old-<sha> dos atuais); (2) roda o /scan no sialia (~30-90 s:
# modelo + dicionário + equivalências); (3) roda UMA consulta de prova e mostra
# o campo `insumos` (o pacote de arquivos que a IA passa a receber).
$ErrorActionPreference = 'Stop'
$wt  = $PSScriptRoot
$bin = Join-Path $env:USERPROFILE '.cargo\bin'
$sha = 'retrieval-57a542f0'

Write-Host "== 1/3 instalando binarios (backup: *.old-$sha) =="
foreach ($n in @('mustard-rt.exe','scan.exe')) {
    $src = Join-Path $wt "target\release\$n"
    if (-not (Test-Path $src)) { throw "faltando build: $src (rode: cargo build --release)" }
    $dst = Join-Path $bin $n
    if (Test-Path $dst) { Move-Item -LiteralPath $dst -Destination "$dst.old-$sha" -Force }
    Copy-Item -LiteralPath $src -Destination $dst
    Write-Host "  instalado: $n"
}
$tsrc = Join-Path $wt 'apps\translate\target\release\mustard-translate.exe'
if (-not (Test-Path $tsrc)) { throw "faltando build: $tsrc (rode: cargo build --release em apps\translate)" }
$tdst = Join-Path $bin 'mustard-translate.exe'
if (Test-Path $tdst) { Move-Item -LiteralPath $tdst -Destination "$tdst.old-$sha" -Force }
Copy-Item -LiteralPath $tsrc -Destination $tdst
Write-Host "  instalado: mustard-translate.exe (tradutor local; modelo baixa 1x no primeiro uso)"

Write-Host "`n== 2/3 scan do sialia (modelo + dicionario + equivalencias) =="
Set-Location C:\Atiz\sialia
$sw = [Diagnostics.Stopwatch]::StartNew()
& mustard-rt run scan --full 2>&1 | Select-Object -Last 6
$sw.Stop()
Write-Host ("  scan total: {0} s" -f [math]::Round($sw.Elapsed.TotalSeconds,1))
foreach ($f in @('grain.model.json','grain.dictionary.json','grain.equivalences.json')) {
    $p = Join-Path 'C:\Atiz\sialia\.claude' $f
    if (Test-Path $p) { $i = Get-Item $p; Write-Host ("  {0}  {1} KB" -f $f, [math]::Round($i.Length/1KB,0)) }
    else { Write-Host "  AVISO: $f nao foi gerado" }
}

Write-Host "`n== 3/3 consulta de prova (o que a IA recebe agora) =="
$raw = & mustard-rt run feature --intent 'listar os planos de acordo com o canal de venda vinculado ao parceiro' 2>$null | Out-String
$idx = $raw.IndexOf('{')
if ($idx -ge 0) {
    $o = $raw.Substring($idx) | ConvertFrom-Json -Depth 64
    if ($o.gloss) { Write-Host "  gloss automatico: $($o.gloss)" }
    $i = 0
    foreach ($x in @($o.insumos)) { $i++; Write-Host ("  {0,2}. [{1}] {2}" -f $i, $x.source, $x.file) }
    if ($i -eq 0) { Write-Host '  AVISO: campo insumos vazio — verifique se o scan gerou o dicionario.' }
} else { Write-Host '  AVISO: sem JSON na resposta do feature.' }
Write-Host "`nPRONTO. Use o sialia normalmente — o /feature agora entrega os insumos sozinho."
