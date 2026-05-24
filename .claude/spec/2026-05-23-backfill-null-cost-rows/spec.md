# Tactical Fix: backfill de cost NULL em run_usage

## Contexto

Derivado de [[2026-05-22-economia-didatica-e-economias-reais]] e do tactical-fix [[2026-05-23-price-frame-model-fallback]].

O fix anterior do `price_frame` faz com que NOVAS ingestões de spans sem model usem o fallback sonnet. Mas as ~310 linhas já gravadas em `run_usage` continuam com `cost_usd_micros = NULL` — exibem `—` (correto, mas não-acionável) e somam zero no agregado por spec.

Fix: novo subcomando `mustard-rt run backfill-run-usage-cost` que percorre `run_usage`, identifica rows com `cost_usd_micros IS NULL AND (input_tokens > 0 OR output_tokens > 0)`, e aplica a mesma fórmula do `price_frame` com fallback sonnet. Idempotente (só toca NULLs), printa contagem de rows atualizadas.

## Decisão de design

- **Idempotente**: só atualiza onde `cost_usd_micros IS NULL`. Rodar duas vezes não double-conta nada.
- **Não toca tokens**: só `cost_usd_micros`. Os contadores de input/output/cache permanecem como estão (são o source of truth).
- **Pricing model**: se a row tem `model` conhecido, usa pricing dele; se NULL/unknown, sonnet fallback — exatamente a mesma política do `price_frame` refatorado.
- **Single transaction**: UPDATE em lote num único savepoint, fail-open ao nível de transaction (rollback em erro, sai com exit 1).
- **CLI emite JSON estável** (regra de subcomandos `run`): `{"rows_scanned":N,"rows_updated":M,"db_path":"..."}`.

## Arquivos

- `packages/core/src/telemetry/writer.rs` — nova fn `backfill_null_costs(conn) -> Result<BackfillReport>` com a lógica de UPDATE em lote
- `apps/rt/src/run/mod.rs` — adicionar variante `BackfillRunUsageCost` ao enum + match no dispatcher
- `apps/rt/src/run/backfill_run_usage_cost.rs` — novo módulo com `pub fn run()` que abre o telemetry.db do cwd e chama a fn do core

## Tarefas

### Library Agent (core)

- [x] `packages/core/src/telemetry/writer.rs`:
  - struct `BackfillReport { scanned: usize, updated: usize }`
  - `pub fn backfill_null_costs(conn: &Connection) -> Result<BackfillReport>`
  - SELECT rows WHERE cost_usd_micros IS NULL AND (input_tokens > 0 OR output_tokens > 0)
  - Para cada row: aplica fórmula sonnet fallback (consume `economy::estimator::model_pricing_usd_micros_per_million`)
  - UPDATE em batch dentro de single transaction
  - Comentários explicando cada ponto da decisão
- [x] Teste inline: seed 3 rows (NULL cost com tokens, NULL cost sem tokens, cost já preenchido); chamar backfill; assert que só a 1ª foi atualizada

### Runtime Agent (rt)

- [x] `apps/rt/src/run/backfill_run_usage_cost.rs`:
  - `pub fn run()` — abre TelemetryStore do project cwd, chama `backfill_null_costs(store.conn())`, emite JSON stdout
  - Fail-open no abrir (eprintln + exit 0) mas exit 1 em erro de UPDATE
- [x] `apps/rt/src/run/mod.rs`:
  - `mod backfill_run_usage_cost;`
  - `RunCmd::BackfillRunUsageCost` variant (sem args)
  - Match no dispatcher

### Execução

- [x] Build `cargo build -p mustard-core -p mustard-rt`
- [x] Rodar uma vez: `rtk mustard-rt run backfill-run-usage-cost` no cwd do user
- [x] Verificar saída: rows_updated > 0

## Critérios de Aceitação

- [x] AC-1: build core+rt verde — Command: `cargo build -p mustard-core -p mustard-rt`
- [x] AC-2: testes core verdes — Command: `cargo test -p mustard-core --lib`
- [x] AC-3: subcomando registrado — Command: `bash -c "grep -q 'BackfillRunUsageCost' apps/rt/src/run/mod.rs && echo ok"`
- [x] AC-4: fn pública existe — Command: `bash -c "grep -q 'backfill_null_costs' packages/core/src/telemetry/writer.rs && echo ok"`

## Limites

- Não tocar tokens — só cost_usd_micros
- Não fazer migration automática na inicialização (manual via subcomando, sem surpresas)
- Não auto-rodar — usuário invoca explicitamente
- Single transaction, fail-open na abertura, exit 1 em UPDATE error
