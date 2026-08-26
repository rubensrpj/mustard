## Terceira revisao: APPROVED — 0 criticos

AC-1 PASS (campo: 8 pendentes, os 6 fixtures sumiram). AC-2 PASS (fim-a-fim com binario real em repo descartavel: mensagem "the tree is clean again", status vazio, commit carregando model + dictionary). AC-3 PASS (com `M work.txt` o portao nao mexeu em nada). AC-4 PASS (build + clippy limpos, suites verdes). Verificado tambem: `scan scan` grava exatamente model + dictionary, e as chaves do `run upsert` seguem inalteradas.

### Nao-bloqueantes — e o que foi feito

1. MAIOR base_gate.rs:180 — `census_refresh_due` decidia visibilidade pelo modo de instalacao, cuja premissa e falsa NESTE repositorio (marcas privadas + censo rastreado). CORRIGIDO: a decisao passa a perguntar se o git VE os arquivos (regra de ignore), o mesmo fato que `record_written_path` julga. Dois testes novos travam os dois lados.
2. MAIOR project_seed.rs:646 — `run upsert` commitava o selo na branch do operador sem narrar nada; a mitigacao de nomear a branch so existia no caminho `mustard init`. CORRIGIDO: `UpsertReport` ganha `stampBranch`, ausente quando nao houve commit, entao a forma do JSON comum nao muda.
3. MINOR base_gate.rs:310 — sidecar ausente faria `git add` abortar inteiro e levar o modelo junto. CORRIGIDO: so caminhos que existem entram na lista.
4. MINOR base_gate.rs:629 — o teste do AC-3 exercita `record_census` direto; `TreeNotClean` e inalcancavel pela porta no caminho compartilhado. NAO corrigido: e observacao sobre onde o teste toca, e o caminho que ele prova e o que carrega a regra.
