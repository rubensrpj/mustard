---
name: rt-run-subcommand-pattern
description: "Pattern for adding a run-face utility subcommand to mustard-rt. Use when porting a JS/bun script to Rust, adding a new mustard-rt run subcommand, writing a CLI utility that emits JSON, or adding a new pipeline automation step. Even if the user just says 'add a run command' or 'port this script'."
source: scan
---
<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->

## Convention

- Each ported script lives in its own file under `src/run/`.
- Exports exactly one public entry point: `pub fn run(...)` returning `()`.
- Takes all inputs as typed arguments (never reads stdin).
- Prints its result to stdout directly — byte-stable JSON when the output is machine-parsed.
- Declared as `mod my_script;` in `src/run/mod.rs`.
- Added as a variant to the `RunCmd` enum in `src/run/mod.rs`.
- Dispatched in `run::dispatch` via a match arm.
- For JSON output: `serde::Serialize` structs + `serde_json::to_string_pretty` + `println!`.
- For harness event emission: use `SqliteEventStore::for_project` + `store.append(...)` with `let _ = ...` (fail-open telemetry).
- Helper env values (project dir, session id): `crate::run::env::{project_dir, session_id}`.
- Timestamp helper: `crate::util::now_iso8601()`.

## Real examples in this codebase

- `apps/rt/src/run/emit_event.rs` — minimal, generic event emitter
- `apps/rt/src/run/emit_phase.rs` — fixed event shape (`pipeline.phase`)
- `apps/rt/src/run/sync_detect.rs` — complex JSON output, SHA-256, role scoring
- `apps/rt/src/run/sync_registry.rs` — entity scanner, multi-language
- `apps/rt/src/run/qa_run.rs` — executes AC commands, emits `qa.result`
- `apps/rt/src/run/security_scan.rs` — file tree scanner, JSON/human report
- `apps/rt/src/run/otel/collector.rs` — long-lived HTTP server (synchronous, `tiny_http`)
- `apps/rt/src/run/mod.rs` — full `RunCmd` enum + dispatch table

## References

See `apps/rt/.claude/skills/rt-run-subcommand-pattern/references/examples.md` for verbatim code extracts.
