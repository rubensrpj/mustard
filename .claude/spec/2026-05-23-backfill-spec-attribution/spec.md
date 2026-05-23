# Tactical Fix: backfill de `spec` em run_usage via run_attribution

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-23T02:30:00Z
### Lang: pt
### Parent: [[2026-05-22-economia-didatica-e-economias-reais]]

## Contexto

Derivado de [[2026-05-22-economia-didatica-e-economias-reais]].

O dashboard mostra só 1 spec ("`2026-05-21-tf-detail-uses-speccard`") na tabela "Custo estimado por spec / onda", embora muitos outros pipelines tenham rodado neste projeto. Investigação revelou a causa raiz:

`packages/core/src/telemetry/migrate.rs::migrate_from_mustard_db` (chamado pelo `run_migration_once` no startup do OTEL collector) **copia** rows do legado `mustard.db` para `run_usage` no `telemetry.db`, mas **não chama** `stamp_attribution` durante a cópia. Resultado: 297 rows entraram em `run_usage` com `spec = NULL` mesmo quando o `run_attribution` tinha (e tem) o stamp correto.

A função `stamp_attribution` (em `apps/rt/src/run/otel/collector.rs:247`) só é aplicada na rota viva da OTEL — não na migração. Por isso o dashboard fica "amnésico" das specs já trabalhadas.

Fix: novo subcomando `mustard-rt run backfill-run-usage-spec` que percorre rows com `spec IS NULL`, faz lookup no `run_attribution` por `(session_id, ts)`, e UPDATE-a `spec`, `wave_id`, `agent_id` quando encontra match. Mesma lógica do `stamp_attribution` mas em batch retroativo.

## Decisão de design

- **Filtro de candidatos**: `spec IS NULL AND session_id IS NOT NULL` — sem session, impossível atribuir.
- **2-tier lookup**: igual ao `stamp_attribution`:
  1. Primary: `(session_id, tool_use_id)` quando a row tem tool_use_id
  2. Fallback session-only: `lookup_attribution_by_session(conn, session, before_ts)` — pega o stamp mais recente <=ts da row
- **Idempotente**: só toca `spec IS NULL`. Re-rodar não double-atribui.
- **Single transaction**: rollback em erro, fail-open na abertura do store.
- **Output JSON**: `{rows_scanned, rows_updated_primary, rows_updated_fallback, rows_unmatched, db_path}`.

## Arquivos

- `packages/core/src/telemetry/writer.rs` — nova fn `backfill_null_spec(conn) -> Result<SpecBackfillReport>` que faz SELECT candidatos + UPDATE por session/tool_use_id
- `apps/rt/src/run/backfill_run_usage_spec.rs` — novo módulo `pub fn run()` 
- `apps/rt/src/run/mod.rs` — variante `BackfillRunUsageSpec`

## Tarefas

### Library Agent (core)

- [x] `packages/core/src/telemetry/writer.rs`:
  - struct `SpecBackfillReport { scanned, updated_primary, updated_fallback, unmatched }` (Serialize)
  - `pub fn backfill_null_spec(conn) -> Result<SpecBackfillReport>` — SELECT span_id, session_id, tool_use_id, started_at WHERE spec IS NULL AND session_id IS NOT NULL
  - Para cada candidato: tenta `lookup_attribution(session, tool_use_id)` primeiro; fallback para `lookup_attribution_by_session(session, before_ts=started_at)`
  - UPDATE row com spec/wave_id/agent_id encontrados, dentro de single transaction
  - Comentários em cada decisão (mirror do stamp_attribution)
- [x] Teste inline: seed 3 rows + 2 attribution stamps, asserta primary/fallback/unmatched

### Runtime Agent (rt)

- [x] `apps/rt/src/run/backfill_run_usage_spec.rs`: análogo a `backfill_run_usage_cost.rs`, abre TelemetryStore + chama backfill_null_spec + emite JSON
- [x] `apps/rt/src/run/mod.rs`: `mod backfill_run_usage_spec;`, `RunCmd::BackfillRunUsageSpec` variant, match

### Execução

- [x] Build `cargo build -p mustard-core -p mustard-rt`
- [x] Rodar `rtk mustard-rt run backfill-run-usage-spec` no cwd
- [x] Reabrir dashboard, confirmar múltiplas specs aparecendo

## Critérios de Aceitação

- [x] AC-1: build core+rt verde — Command: `cargo build -p mustard-core -p mustard-rt`
- [x] AC-2: testes core verdes — Command: `cargo test -p mustard-core --lib`
- [x] AC-3: subcomando registrado — Command: `bash -c "grep -q 'BackfillRunUsageSpec' apps/rt/src/run/mod.rs && echo ok"`
- [x] AC-4: fn pública existe — Command: `bash -c "grep -q 'backfill_null_spec' packages/core/src/telemetry/writer.rs && echo ok"`

## Limites

- Não tocar `usage_totals` (sem dimensão spec)
- Não popular run_attribution — apenas consume o que já existe lá
- Single transaction; exit 1 em UPDATE error
