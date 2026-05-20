---
name: core-lenient-serde-model
description: "Lenient serde struct with raw catch-all for external JSON. Use when adding a new boundary model that accepts harness stdin or pipeline-state JSON, deserializing any external JSON that may grow new fields, or keeping a model forward-compatible without breaking old code. Even if the user just says 'add a model for the hook input' or 'don't break on new fields'."
source: scan
---
<!-- mustard:generated at:2026-05-19T00:00:00Z role:general -->

## Convention

- Boundary structs that accept external JSON carry `#[serde(flatten)] pub raw: Value` as the last field.
- Known fields are typed with `#[serde(default, skip_serializing_if = "...")]` so absent fields do not error.
- Enums that may grow use `#[non_exhaustive]`. Closed-vocabulary enums (e.g. `Mode`) do not.
- Tool-specific or event-specific sub-payloads stay `Value` (untyped) — a struct would enumerate every tool.
- `serde(rename = "camelCase")` is used per-field when the JSON key differs from the Rust name.
- Tests assert round-trip fidelity and that unknown fields land in `raw`, not an error.

## Real examples in this codebase

- `packages/core/src/model/contract.rs` — `HookInput` with `raw: Value` catch-all
- `packages/core/src/model/pipeline.rs` — `PipelineState` with `raw: Value` catch-all
- `packages/core/src/model/event.rs` — `HarnessEvent` with `payload: Value` for event-specific data
- `packages/core/src/model/contract.rs` — `Verdict` as `#[non_exhaustive]` enum with `#[serde(tag = "decision")]`

## References

See `packages/core/.claude/skills/core-lenient-serde-model/references/examples.md` for verbatim code excerpts.
