---
name: rt-check-pattern
description: Use when adding or refactoring a doctor/audit check that scans project state under `.claude/` and emits a deterministic, fail-open JSON report.
tags: [add, refactor]
appliesTo: [check]
scope: [code-editing]
source: scan
metadata:
  generated_by: scan
  cluster:
    label: check
---

# check pattern

## Purpose

The `check` modules are read-only linters run by `mustard-rt run doctor --check <name>` (or a dedicated `run <name>-check` subcommand). Each one walks a slice of project state — capability docs, spec dirs, source-of-truth markdown — and builds a serializable report of findings; doctor never deletes and a check never blocks, it reports so the maintainer decides. Every check is fail-open by construction: an unreadable or malformed input is skipped and recorded in a `scanned_errors` vector, the scan continues, and the process never panics (`clippy::unwrap_used`/`expect_used` are `deny` outside `#[cfg(test)]`). Output is byte-stable because `insta` snapshots and gates compare it: every vector is sorted before serializing, ratios are fixed-point integers (×1000, no float), paths are repo-relative, and there are no timestamps. When the check cannot judge at all (e.g. no grain model yet), it is a silent no-op — `run` returns `None` rather than reporting false positives.

## Convention

- Folder: `apps/rt/src/commands/doctor/`
- Suffix: `check` (file `{name}_check.rs`)
- Extension: `.rs`
- Declares: functions (`pub fn run(root: &Path) -> Report` or `-> Option<Report>`)
- Count: 18

## How to apply

To add a new `check`:

- Create `apps/rt/src/commands/doctor/{name}_check.rs` and declare it as `pub mod {name}_check;` in `apps/rt/src/commands/doctor/mod.rs`.
- Open with a `//!` module doc naming the exact invocation (`mustard-rt run doctor --check <name>`), the rationale, and the fail-open story — every exemplar does.
- Expose `pub fn run(root: &Path) -> Report`; return `Option<Report>` only when a missing precondition makes the check unjudgeable (silent no-op, like `capability_drift_check` without a grain model).
- Shape the report as `#[derive(Debug, Serialize, PartialEq, Eq)]` structs with an `ok: bool`, sorted `Vec`s, and a `scanned_errors: Vec<String>` carrying the fail-open evidence. Keep it byte-stable: sort everything, no floats, no timestamps, no absolute paths.
- Keep detection pure and testable: a private no-IO function (`detect_*`) fed by an injected known-set/input, plus a `build_report` that does the filesystem walk via `mustard_core::io::fs` and collects+sorts entries so iteration order is deterministic regardless of readdir order.
- Advisory only: emit `HarnessEvent`s via `crate::shared::events::route::emit` if the finding deserves telemetry (errors swallowed), but never error the doctor run. Blocking, if any, belongs to the gate that consumes the report (e.g. `docs_stale_check` + `MUSTARD_DOCS_AUDIT_MODE=strict`).
- If it is a standalone `run` subcommand, register it TWICE in the family `cli.rs`: the enum variant AND the `dispatch()` arm — forgetting the second compiles but the command disappears (`tests/run_command_surface.rs` locks the list).
- Do not add a `regex` crate — `mustard-rt` carries none; write byte-wise/split matchers as `docs_stale_check` does.
- Tests live in-file under `#[cfg(test)] mod tests`, seeding `.claude/` fixtures in a `tempfile::tempdir` and asserting byte-stability across runs.

## Examples

- Ref: apps/rt/src/commands/doctor/capability_drift_check.rs
- Ref: apps/rt/src/commands/doctor/superseded_check.rs
- Ref: apps/rt/src/commands/doctor/docs_stale_check.rs
