---
name: telemetry-span-to-run-rename
description: core telemetry/economy "span" identifiers renamed to "run" (2026-05-22); SpanRecord KEPT as OTLP wire type; columns kept; rt has stale doc-comment refs
metadata:
  type: project
---

After the telemetry-separation refactor (`mustard_core::telemetry` over `.harness/telemetry.db` with `usage_totals`/`run_usage`/`run_attribution`), the stored-row identifiers that still said "span" were aligned to `run` (2026-05-22).

Renamed in `packages/core` (behavior-preserving):
- `economy::writer::record_span` → `record_run` (public, re-exported from `economy/mod.rs`). rt only referenced it in DOC comments, never called it, so rt still built.
- `store::sqlite_store::SpanRow` → `RunRow` (public but no cross-crate consumer; not re-exported at crate root).
- `store::sqlite_store::SqliteEventStore::spans()` → `runs_by_spec()` (no cross-crate consumer).

**KEPT** (the rule: "span" stays only at the OTLP/wire ingestion boundary):
- `economy::model::SpanRecord` (+ alias `ApiCostFrame`) — the OTLP-translated incoming record, struct-init'd by rt (`tracker.rs`, `otel/collector.rs`) and produced by `economy::sources::otel::{ingest, translate_span}` / `transcript.rs`. Renaming would ripple cross-crate.
- `economy::writer::record_api_cost` (cost, not the table) and the private `span_to_run` mapper (span→run is accurate).
- All SQL **column** names `span_id` / `parent_span_id` in `run_usage` / `schema.sql` — renaming a PK column is a schema migration, out of scope.

**Why:** identifier names were misleading after tables went clean. **How to apply:** if you DO want column renames later, that's a `telemetry/migrate.rs` schema bump, not a Rust rename. Stale DOC-only refs remain in `apps/rt/src/hooks/tracker.rs` (`record_span`, `spans`) and `apps/rt/src/mcp/mod.rs` (`SpanRow`) — harmless to compilation, fix opportunistically.

Also added retention API in `telemetry::writer`: `prune_older_than(conn, cutoff_ts_ms) -> Result<usize>` + `prune_older_than_days(conn, days, now_ts_ms)`. Deletes `run_usage.started_at` and `usage_totals.updated_at` strictly older than cutoff (NULL ts kept), one transaction, fail-open at caller. Reachable as `mustard_core::telemetry::writer::prune_older_than` (writer is `pub mod`). To be wired into rt `session_cleanup` separately. See [[core-fs-seam]].
