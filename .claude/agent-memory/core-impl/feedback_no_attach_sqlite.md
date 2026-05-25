---
name: feedback-no-attach-sqlite
description: ATTACH DATABASE is forbidden in packages/core/src/{store,telemetry}/** — caused silent lock contention that destroyed ~42 MB of telemetry and produced a v9→v10 test hang
metadata:
  type: feedback
---

`ATTACH DATABASE` is forbidden anywhere under `packages/core/src/store/**` and `packages/core/src/telemetry/**`. The only allowed mention is doc-comments that explain why it's forbidden.

**Why:**
- Historical (WARN-3, 2026-05-22): the v7→v8 migration opened a second connection (`TelemetryStore`) that `ATTACH`ed `mustard.db` by path while the main migration's `conn` already held it. The second reader contended with `conn`, and the source-table probe in `telemetry::migrate` swallowed the resulting busy/lock as "table absent" → the copy silently skipped both tables yet the drop still ran → ~42 MB of telemetry destroyed without being copied.
- Re-inversion fix (attach `telemetry.db` from the `mustard.db` `conn` instead of the reverse) carried the same problem in a different shape: the attached state survived the v7→v8 return, so v9→v10's `DROP TABLE` later contended with the still-attached face of the same file, producing a test hang nobody could diagnose.
- The W5 unification spec already specified "padrão fase-dev — drop limpo, sem migration formal" for this telemetry data; carrying the rows was an unnecessary risk given [[feedback_no_migration_dev_phase]].

**How to apply:**
- v7→v8 is now a no-op version bump. The `claude_code_otel` / `spans` drop moved into v9→v10 alongside the other legacy tables (`events`, `events_fts`, `knowledge`, `metrics_projection`, `savings_records`, `context_cost_frames`, `api_cost_frames`).
- If a future task needs cross-DB data movement, do it OUTSIDE migrations (a `mustard-rt run` subcommand on a separate connection, after the open transaction has committed) — never inside `SqliteEventStore::new`.
- The deleted module was `packages/core/src/telemetry/migrate.rs` — do not bring it back.
- Doc-comments that mention `ATTACH DATABASE` are OK and load-bearing (they keep the rule visible to future agents).
