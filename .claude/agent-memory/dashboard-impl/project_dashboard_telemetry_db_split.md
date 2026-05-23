---
name: dashboard-telemetry-db-split
description: Dashboard src-tauri reads cost/tokens from a separate .harness/telemetry.db (run_usage/usage_totals), NOT mustard.db spans; two cost sources must stay on separate cards
metadata:
  type: project
---

The dashboard's `src-tauri/src/db.rs` + `telemetry.rs` read consumption/cost from a dedicated `.claude/.harness/telemetry.db` (`mustard_core::telemetry`), tables `run_usage` (per-run, internally ESTIMATED `cost_usd_micros`) and `usage_totals` (Claude Code's native billed USD). The legacy mustard.db `spans` table is retired.

**Why:** Wave 3 of `2026-05-22-db-access-repository-and-live-refresh` moved span-based aggregation onto the self-attributed telemetry.db.

**How to apply:**
- Open the `TelemetryStore` ONCE per Tauri command via `db::telemetry_store_for(repo_path)` and pass `Option<&TelemetryStore>` into helpers (`metrics_from_db`, `aggregate_activity_from_db`, `quality_metrics_from_db`, `consumption_*`, `cost_summary`). Open it OUTSIDE/BEFORE the `with_db` closure — that closure holds the mustard.db `DbCache` mutex, and opening telemetry inside it risks a latent deadlock. (Fixed 2026-05-22.)
- `run_usage.wave_id` is a TEXT slug like `"wave-1-core"` — never `parse::<i64>()` it directly; use `wave_num_from_slug` (digits after `"wave-"`).
- TWO cost numbers exist and must NEVER be summed into one KPI: native billed USD (`usage_totals` → `cost_block`/`dashboard_prompt_economy`, shown as "custo medido (Anthropic)" in `EconomySection.tsx`) vs estimated USD (`run_usage` → `dashboard_consumption`/`dashboard_economy_summary`, shown as "Custo USD (estimado) · tokens × tabela" in `AggregateOverview.tsx`/`Economia.tsx`). Verified separate as of 2026-05-22.
- `tests/db_test.rs` uses in-memory SQLite with no telemetry sibling, so token counters are 0 (pass `None`); the legacy `spans` insert in `setup()` is dead w.r.t. token totals.
