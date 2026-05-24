# Tactical Fix: pricing cache-aware (input/cache_creation/cache_read/output rates)

## Contexto

Derivado de [[2026-05-22-economia-didatica-e-economias-reais]].

`price_frame` em `packages/core/src/economy/sources/transcript.rs:217` (e a função `backfill_null_costs` em `writer.rs` que copia a mesma lógica) trata `cache_read` como input em preço cheio:

```rust
let input = input_tokens.unwrap_or(0).saturating_add(cache_read.unwrap_or(0));
```

A Anthropic na verdade cobra:
- `input_tokens`: preço base
- `cache_creation_input_tokens`: **1.25×** preço base (cobrança extra por escrita)
- `cache_read_input_tokens`: **0.10×** preço base (10% — desconto agressivo de hit)
- `output_tokens`: preço de output

Em workload com Claude Code (que tem prefixos enormes cacheados), a maior parte dos tokens é `cache_read`. Tratar como input cheio infla 7× a 10× o custo estimado. Resultado visível no dashboard: card "Custo do projeto (medido)" = $78.48; um único agente `core-impl` aparece com $284.91. Numerador inflado em mais de 3× só pelo erro de cache_read.

Fix: pricing separado por bucket de token. Mesma função em `price_frame` e em `backfill_null_costs` — encapsular num helper compartilhado pra não divergir.

## Decisão de design

- **Helper compartilhado**: `pub fn compute_cost_micros(model, input, cache_creation, cache_read, output) -> Option<i64>` em `economy::estimator` (já tem `model_pricing_usd_micros_per_million` no módulo).
- **Fórmula honesta:**
  ```
  cost = (input × rate_in
        + cache_creation × rate_in × 5/4
        + cache_read × rate_in / 10
        + output × rate_out) / 1_000_000
  ```
  Multiplicadores inteiros (5/4 e 1/10) via aritmética saturating para evitar drift de ponto flutuante.
- **Fallback sonnet** preservado (política do tactical-fix anterior): se model é `None`/desconhecido, usa pricing sonnet.
- **Re-execução do backfill**: após o fix, rodar `mustard-rt run backfill-run-usage-cost` novamente. Como o backfill só toca rows com `cost IS NULL OR cost = 0`, e essas já foram preenchidas com valor inflado, **não vai recalcular**. Precisa filtro adicional ou flag `--force` que recalcule todas as rows independente do estado.
- **Adicionar flag `--force`** ao subcomando que reaplica em TODAS as rows (`UPDATE run_usage SET cost = computed`). Sem `--force`, comportamento idempotente atual.

## Arquivos

- `packages/core/src/economy/estimator.rs` — adicionar `pub fn compute_cost_micros(...)` cache-aware com comentário explicando a fórmula
- `packages/core/src/economy/sources/transcript.rs` — `price_frame` delega para `compute_cost_micros`
- `packages/core/src/telemetry/writer.rs` — `backfill_null_costs` delega para `compute_cost_micros`; novo parâmetro `force: bool` para recalcular rows existentes
- `apps/rt/src/run/backfill_run_usage_cost.rs` — aceitar `--force` flag; passar para core
- `apps/rt/src/run/mod.rs` — `BackfillRunUsageCost { force: bool }` (clap arg)

## Tarefas

### Library Agent (core)

- [x] `economy/estimator.rs`: nova `compute_cost_micros` cache-aware com unit tests
- [x] `economy/sources/transcript.rs::price_frame`: passar a delegar; testes existentes precisam ajustar valores (cache_read agora barato → cost menor)
- [x] `telemetry/writer.rs::backfill_null_costs`: aceitar `force: bool`. Quando force=true, SELECT todas as rows com tokens > 0 (não só NULL/0).
- [x] Atualizar tests que assertam custos específicos
- [x] `cargo build && cargo test -p mustard-core --lib`

### Runtime Agent (rt)

- [x] `apps/rt/src/run/mod.rs`: `RunCmd::BackfillRunUsageCost { #[arg(long)] force: bool }`
- [x] `apps/rt/src/run/backfill_run_usage_cost.rs::run(force)` — repassa para core

### Execução

- [x] `rtk cargo run -p mustard-rt -- run backfill-run-usage-cost --force` — atualizar todas as rows com fórmula correta
- [x] Confirmar via JSON output que rows_updated > 0 (provavelmente >91)

## Critérios de Aceitação

- [x] AC-1: build core+rt verde — Command: `cargo build -p mustard-core -p mustard-rt`
- [x] AC-2: testes core verdes — Command: `cargo test -p mustard-core --lib`
- [x] AC-3: helper público existe — Command: `bash -c "grep -q 'pub fn compute_cost_micros' packages/core/src/economy/estimator.rs && echo ok"`
- [x] AC-4: fórmula respeita cache discount — Command: `bash -c "grep -q '/ 10' packages/core/src/economy/estimator.rs && echo ok"`
- [x] AC-5: flag --force registrada — Command: `bash -c "grep -q 'force' apps/rt/src/run/backfill_run_usage_cost.rs && echo ok"`

## Limites

- Não tocar `usage_totals` (MEDIDO da Anthropic, não-estimado)
- Não mudar UI nesta spec (separada em [[2026-05-23-economia-i18n-migration]])
- Apenas saturating arithmetic (sem floats)
