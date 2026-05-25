---
name: w5-event-route-split
description: W5 mustard-unification — single router event_route::emit classifies pipeline.* → SQLite, anything else → per-spec NDJSON via event_writer_ndjson; consumer tests reading non-pipeline from SQLite are out of date
metadata:
  type: project
---

W5 wave 5 of `2026-05-24-mustard-unification` landed a per-spec NDJSON sink + single
classifier:

- `apps/rt/src/run/event_route.rs` — `emit(project_dir, &event)`. Classifies by
  `event.starts_with("pipeline.")`. Pipeline → SQLite via `EventSink::append`.
  Everything else → `event_writer_ndjson::write_event` under
  `<project>/.claude/spec/{name}/[wave-N-{role}/]events/`. Falls back to
  `.claude/.session/<slug>/events/` when no spec resolves.
- `wave_role` reads `MUSTARD_ACTIVE_WAVE` + `MUSTARD_ACTIVE_WAVE_ROLE` env vars.
- All hook + run-face emitters of non-pipeline events now call
  `event_route::emit` instead of `SqliteEventStore::for_project(X).and_then(|s| s.append(&event))`.
- The two ex-`#![allow(dead_code)]` modules (`event_writer_ndjson`, `blob_spill`)
  are now live.

**Why:** Pre-W5 every event went to SQLite (`events` table). The W5 split keeps
only `pipeline.*` in SQLite (`pipeline_events`) and routes the hot-path event
log to NDJSON. The classifier centralises the routing so future hooks need to
call exactly one thing.

**How to apply:** When adding any new event emitter in `apps/rt/`, use
`crate::run::event_route::emit(project_dir, &harness_event)` — do NOT call
`store.append(&event)` directly unless you specifically need SQLite (pipeline
lifecycle / amend-window writes).

## Known follow-up debt (out of scope for this wave)

The W5 SQLite split (`sqlite_store.rs::EventSink::append` silently drops
non-pipeline events) left several CONSUMERS reading from SQLite for events that
now live in NDJSON. They must be ported to read NDJSON. The known broken
consumer reads:

- `hooks::close_gate::find_last_qa_result` — reads `qa.result` from SQLite
  replay. Now in NDJSON. Breaks the QA gate that gates CLOSE.
- `hooks::knowledge::spec_has_retry_events` — `has_event_for_spec("retry.attempt", …)`
  on `pipeline_events` table. Now in NDJSON. Breaks `retry.attempt`
  idempotence.
- `hooks::session_start::*` knowledge injection reads `knowledge_patterns`
  table — fine on its own, but the "knowledge.captured" / "lesson" event
  feeders that populate it go to NDJSON now and are not back-ingested.
- `mcp::tests::query_events_filters_by_spec_event_and_since` — MCP server
  reads `tool.use` / `decision` from SQLite for the timeline view.
- `run::memory_cross_wave` — reads `agent.memory` from SQLite. Now in NDJSON.
- `run::metrics_wave_status` — aggregates by reading SQLite events.

Pre-existing test failures from the W5 sqlite split (21 of them as of
2026-05-24): `close_gate_allows_when_qa_*`, `retry_attempt_*`,
`memory_injection_*`, `mcp::tests::*`, `event_spec_field_survives_sqlite_round_trip`,
`memory_cross_wave::*`, `metrics_wave_status::aggregates_per_wave`,
`rebuild_specs::*`, `each_kind_appended_once_with_correct_event_name`
(hygiene.* + pipeline.economy.* in KNOWN_KINDS hit the same `!pipeline.`
drop).

The fix shape is a parallel `NdjsonReader` in `mustard_core` + porting each
consumer; tracked separately. None of these were introduced by W5 wire-up;
they were latent in the sqlite_store split.
