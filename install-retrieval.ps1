# install-retrieval.ps1 — instala a busca de código e liga no sialia (1 comando).
# O que faz: (1) instala mustard-rt + scan em ~/.cargo/bin (com backup
# .old-<sha> dos atuais); (2) roda o /scan no sialia (~30-90 s: o modelo);
# (3) roda UMA consulta de prova e mostra os arquivos que ela aponta.
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

Write-Host "`n== 2/3 scan do sialia (modelo) =="
Set-Location C:\Atiz\sialia
$sw = [Diagnostics.Stopwatch]::StartNew()
& mustard-rt run scan --full 2>&1 | Select-Object -Last 6
$sw.Stop()
Write-Host ("  scan total: {0} s" -f [math]::Round($sw.Elapsed.TotalSeconds,1))
foreach ($f in @('grain.model.json')) {
    $p = Join-Path 'C:\Atiz\sialia\.claude' $f
    if (Test-Path $p) { $i = Get-Item $p; Write-Host ("  {0}  {1} KB" -f $f, [math]::Round($i.Length/1KB,0)) }
    else { Write-Host "  AVISO: $f nao foi gerado" }
}

Write-Host "`n== 3/3 consulta de prova (o que a IA recebe agora) =="
$raw = & mustard-rt run map search --query 'listar os planos de acordo com o canal de venda vinculado ao parceiro' 2>$null | Out-String
$idx = $raw.IndexOf('{')
if ($idx -ge 0) {
    $o = $raw.Substring($idx) | ConvertFrom-Json -Depth 64
    $i = 0
    foreach ($x in @($o.files)) { $i++; Write-Host ("  {0,2}. {1}" -f $i, $x.path) }
    if ($i -eq 0) { Write-Host '  AVISO: nenhum arquivo apontado — verifique se o scan gerou o modelo.' }
} else { Write-Host '  AVISO: sem JSON na resposta do map.' }
Write-Host "`nPRONTO. Use o sialia normalmente — o mapa agora entrega os arquivos sozinho."
