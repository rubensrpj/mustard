# Tactical Fix: qa-run parser aceita ACs com dash interno (AC-W4-1, AC-G-7, etc.)

## Contexto

Tactical fix derivado de [[2026-05-23-dashboard-design-system]] (surface em wave-4 QA).

O parser de Acceptance Criteria do `mustard-rt run qa-run` reconhece IDs no formato `AC-[A-Za-z0-9]+` (vide memory `project_qa_run_parser_idiom_agnostic.md` — suportou `AC-G1..G7` desde 2026-05-20). Não cobre IDs com **múltiplos segmentos separados por dash**, ex.: `AC-W4-1`, `AC-TF-3`, `AC-W4-10`. Hoje quando se aponta `qa-run --spec X/wave-4-ui` com ACs `AC-W4-N`, o parser falha com warning `Acceptance Criteria section found but no parseable AC items` e retorna `overall: skip`.

Solução: estender o regex de `parse_ac_line` (em `apps/rt/src/`) para aceitar ID multi-segmento (`AC(-[A-Za-z0-9]+)+`). Comportamento existente preservado (IDs single-segment como `AC-1`, `AC-G7` continuam matchando).

## Critérios de Aceitação

- [x] AC-TF-P1: parser reconhece `AC-W4-1` como ID válido — Command: `node -e "const {execSync}=require('child_process');const out=execSync('cargo run --quiet -p mustard-rt -- run qa-run --spec 2026-05-23-dashboard-design-system/wave-4-ui --format json',{encoding:'utf8',cwd:process.cwd()});const r=JSON.parse(out.split('\\n').filter(l=>l.startsWith('{')||l.startsWith('  ')).join('\\n')||out);if(r.payload.criteria.length===0){console.error('no AC parsed');process.exit(1)}if(!r.payload.criteria.some(c=>c.id==='AC-W4-1')){console.error('AC-W4-1 not in criteria');process.exit(2)}console.log('ok')"`
- [x] AC-TF-P2: IDs single-segment ainda parseiam (regressão) — Command: `node -e "const {execSync}=require('child_process');const out=execSync('cargo run --quiet -p mustard-rt -- run qa-run --spec 2026-05-23-dashboard-design-system --format json',{encoding:'utf8'});const r=JSON.parse(out.split('\\n').filter(l=>l.startsWith('{')||l.startsWith('  ')).join('\\n')||out);if(!r.payload.criteria.some(c=>c.id==='AC-1')){console.error('AC-1 regressed');process.exit(1)}console.log('ok')"`
- [x] AC-TF-P3: teste unitário cobre IDs `AC-W4-1`, `AC-TF-3`, `AC-G1`, `AC-1` — Command: `cargo test -p mustard-rt --quiet parse_ac_line 2>&1 | grep -E "(test result|passed)" | head -3`

## Arquivos

- `apps/rt/src/run/qa_run.rs` (ou wherever `parse_ac_line` mora — confirmar via Grep)
- Possível arquivo de testes adjacente (mesmo módulo ou `tests/`)

## Tarefas

- [ ] Grep `parse_ac_line` em `apps/rt/src/` — localizar função e regex atual.
- [ ] Estender o regex para `AC(-[A-Za-z0-9]+)+` (aceita dash interno; preserva single-segment).
- [ ] Adicionar/atualizar teste cobrindo: `AC-1`, `AC-G1`, `AC-W4-1`, `AC-TF-3`, `AC-W4-10`.
- [ ] `cargo build -p mustard-rt --release` — recompilar.
- [ ] `cargo install --path apps/rt --force` — atualizar binário no PATH (vide memory `project_mustard_rt_stale_binary.md`).
- [ ] Re-rodar `mustard-rt run qa-run --spec 2026-05-23-dashboard-design-system/wave-4-ui` — confirma overall != skip e 10 ACs parseados.

## Limites

Editar APENAS:
- `apps/rt/src/run/qa_run.rs` (e adjacentes do mesmo módulo se a fn estiver split)
- Arquivos de teste do mesmo módulo

Não tocar: outros modules de `apps/rt/`, `apps/cli`, `apps/dashboard`, `packages/core`.

## Checklist

- [x] Regex estendido
- [x] Teste cobre IDs multi-dash
- [x] `cargo test -p mustard-rt` verde
- [x] `cargo build -p mustard-rt --release` verde
- [x] Binário reinstalado via `cargo install --path apps/rt --force`
- [x] `mustard-rt run qa-run --spec 2026-05-23-dashboard-design-system/wave-4-ui` parseia 10 ACs
