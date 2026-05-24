# W12 — Telemetry perf followup + Economy dashboard wiring

## Contexto

Duas frentes convergem:

1. A spec `2026-05-22-telemetry-separation` está `Close/Completed`, mas review/qa subdirs estão `Plan/Active`. Esta onda fecha.
2. A economia produzida por toda a mega-spec tem que ser visível em `/economia` em tempo real. Sem isso, "economia" é folclore. Nesta onda, os subcomandos `economy *` (entregues em W6) ganham wire ao dashboard, e o pipeline de medição (`baseline + post → reconcile → savings`) é instrumentado.

## Tarefas

### T12.1 — Fechar review/qa de telemetry-separation

- [ ] Rodar review formal: verificar `usage_totals` reduzida (~-90% espaço vs `claude_code_otel` antigo); cada run nasce atribuído sem JOIN. Emit `review.complete`.
- [ ] Rodar QA: ACs originais ainda passam. Emit `qa.complete`.
- [ ] Emit `pipeline.status: archived` para `2026-05-22-telemetry-separation` + sub-waves.

### T12.2 — Audit telemetry.db performance

- [ ] `EXPLAIN QUERY PLAN` em cada query hot do dashboard (telas de economia/telemetria). Documentar resultado em `packages/core/src/telemetry/query_audit.md`.
- [ ] Criar índices ausentes: `run_usage(spec, wave)`, `run_usage(at)`, `run_usage(model)` se hot path está em full scan.
- [ ] Adicionar `PRAGMA optimize` no `open()` do telemetry store (paridade com `mustard.db`).

### T12.3 — Estender `db-maintain`

- [ ] Flag `--telemetry-only` em `apps/rt/src/run/db_maintain.rs` para VACUUM/prune separado do `mustard.db`.
- [ ] Flag `--prune-older-than <Nd>` (default 90d) que apaga rows `run_usage` antigas. Opt-in `--keep-all`.
- [ ] Default seguro: dry-run mostra o que seria apagado.

### T12.4 — Schema das tabelas de economia

- [ ] Adicionar em `packages/core/src/telemetry/schema.sql`:
  ```sql
  CREATE TABLE IF NOT EXISTS economy_baselines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    wave_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    baseline_tokens INTEGER NOT NULL,
    baseline_duration_ms INTEGER,
    captured_at TEXT NOT NULL,
    source TEXT
  );
  CREATE INDEX IF NOT EXISTS idx_economy_baselines_wave_op
    ON economy_baselines(wave_id, operation);

  CREATE TABLE IF NOT EXISTS economy_savings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    wave_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    baseline_id INTEGER,
    post_tokens INTEGER NOT NULL,
    post_duration_ms INTEGER,
    savings_tokens INTEGER NOT NULL,
    savings_pct REAL,
    captured_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_economy_savings_wave_op
    ON economy_savings(wave_id, operation);
  ```

### T12.5 — Wire dos subcomandos `economy *` ao dashboard

- [ ] `mustard-rt run economy capture-baseline --operation X --wave Y [--from-history]` (W6 entregou) — quando rodado pré-wave, grava row em `economy_baselines`.
- [ ] `mustard-rt run economy reconcile --wave W --since <ts>` — agrega events `pipeline.economy.*` da janela, calcula `savings_tokens = baseline - post`, grava em `economy_savings`.
- [ ] `mustard-rt run economy report --format json|table --wave all|<id>` — exporta histórico completo.

### T12.6 — Dashboard `/economia` ganha aba "Mustard Unification Savings"

- [ ] Card "Unification savings (total)": soma cumulativa de `savings_tokens` desde início da mega-spec.
- [ ] Tabela "Per-wave breakdown": linha por wave × operação, com baseline, post, delta absoluto, delta %.
- [ ] Sparkline temporal: economia diária acumulada ao longo das semanas de execução das waves.
- [ ] Comparação cruzada: breakdown por subprojeto (cli, rt, core, dashboard).
- [ ] Tauri command `read_economy_savings` em `apps/dashboard/src-tauri/src/commands/economy.rs` (novo arquivo).

### T12.7 — Backfill via `--from-history`

- [ ] `economy capture-baseline --from-history --operation X --window-days 30` calcula mediana de `run_usage` no telemetry.db para essa operação nos últimos 30d. Permite gravar baseline retroativo para W2-W5 que rodaram antes do pipeline completo.

### T12.8 — 5 ACs globais de metrification (verificáveis aqui)

- [ ] AC-Metric-1: Toda wave que substituiu operação de IA por Rust emite ao menos 1 `pipeline.economy.baseline.captured` event antes de iniciar.
- [ ] AC-Metric-2: Cada subcomando novo (W6) emite `pipeline.economy.operation.invoked` em ao menos 1 caminho de execução.
- [ ] AC-Metric-3: `mustard-rt run economy reconcile --wave WN` produz row em `economy_savings` para cada wave de W2 a W11.
- [ ] AC-Metric-4: Dashboard `/economia` mostra "Unification savings (total)" não-zero após pelo menos W2+W4+W5+W6 fecharem.
- [ ] AC-Metric-5: Tabela `economy_baselines` + `economy_savings` exportável via `mustard-rt run economy report --format json --wave all` para auditoria.

## Files

- `packages/core/src/telemetry/schema.sql` (T12.4)
- `packages/core/src/telemetry/store.rs` (PRAGMA optimize + queries com índices)
- `packages/core/src/telemetry/query_audit.md` (novo — documentação dos EXPLAIN PLANs)
- `apps/rt/src/run/db_maintain.rs` (T12.3 flags)
- `apps/rt/src/run/economy_capture_baseline.rs` (T12.5)
- `apps/rt/src/run/economy_reconcile.rs` (T12.5)
- `apps/rt/src/run/economy_report.rs` (T12.5)
- `apps/dashboard/src/pages/Economia.tsx` (T12.6 — nova aba)
- `apps/dashboard/src-tauri/src/commands/economy.rs` (novo — `read_economy_savings`)
- `.claude/spec/2026-05-22-telemetry-separation/{review,qa}/spec.md` (fechar Stage/Outcome via meta.json após W3)

## Critérios de Aceitação

- [ ] AC-W12-1: `2026-05-22-telemetry-separation` review e qa subdirs com `Outcome: Completed`. Command: `node -e "const fs=require('fs');for(const sub of ['review','qa']){const m=JSON.parse(fs.readFileSync('.claude/spec/2026-05-22-telemetry-separation/'+sub+'/meta.json','utf8'));if(m.outcome!=='Completed')process.exit(1)}"`
- [ ] AC-W12-2: Tabelas `economy_baselines` e `economy_savings` existem em `telemetry.db`. Command: `node -e "const{execSync}=require('child_process');for(const t of ['economy_baselines','economy_savings']){const out=execSync('sqlite3 .claude/.telemetry/telemetry.db \".schema '+t+'\"',{encoding:'utf8'});if(!out.includes('CREATE TABLE'))process.exit(1)}"`
- [ ] AC-W12-3: `economy reconcile --wave W5` cria row em `economy_savings`. Command: fixture.
- [ ] AC-W12-4: Dashboard `/economia` mostra card "Unification savings (total)" não-zero. Command: manual.
- [ ] AC-W12-5: `economy report --format json --wave all` retorna JSON com >= 10 entries (W2-W11). Command: `rtk mustard-rt run economy report --format json --wave all | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!Array.isArray(j.waves)||j.waves.length<10)process.exit(1)})"`
- [ ] AC-W12-6: Índices criados na telemetry.db. Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.telemetry/telemetry.db \".indexes run_usage\"',{encoding:'utf8'});if(!out.includes('idx_'))process.exit(1)"`
- [ ] AC-W12-7: `db-maintain --telemetry-only --prune-older-than 90d --dry-run` lista o que seria apagado. Command: fixture.

## Notas

- Bloqueia W13 (close-and-archive).
- Sub-comandos `economy *` foram entregues em W6; W12 só faz wire ao dashboard.
- Baseline retroativo possível via `--from-history` (telemetry.db já tem `run_usage` desde a separation).
