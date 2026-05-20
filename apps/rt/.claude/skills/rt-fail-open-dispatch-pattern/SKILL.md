---
name: rt-fail-open-dispatch-pattern
description: "Pattern for the mustard-rt harness wire protocol and fail-open dispatch. Use when working on the dispatcher, protocol encoding, verdict folding, mode application, or the stdin/stdout hook contract. Even if the user just says 'hook protocol', 'verdict encoding', or 'how does dispatch work'."
source: scan
---
<!-- mustard:generated at:2026-05-19T00:00:00.000Z role:general -->

## Convention

- Dispatcher owns fail-open — modules never handle their own errors.
- `Check::evaluate` returns `Result<Verdict, Error>`; `Err` → `Allow` (dispatcher absorbs).
- `Observer::observe` returns `()`; errors are swallowed inside the method.
- Process always exits `0`; `Deny` is signalled in the JSON body, not via exit code.
- Verdict folding: `Outcome::fold(verdict)` accumulates the worst verdict; warnings accumulate separately.
- Mode application in dispatcher: `Warn` mode downgrades `Deny → Warn`; `Off` skips the module entirely.
- Wire protocol: stdin = single JSON object parsed as `HookInput`; stdout = optional `{ "hookSpecificOutput": { ... } }`.
- Silent allow: no stdout at all when outcome is bare `Allow` with no warnings.
- `Inject` verdict → `additionalContext` field (advisory context injected into the agent).
- `Rewrite` verdict → `updatedInput` field (tool input is modified).
- `Deny` verdict → `permissionDecision: "deny"` + `permissionDecisionReason`.

## Real examples in this codebase

- `apps/rt/src/dispatch.rs` — `run_event`, `run_check`, `run_module`, `apply_mode`
- `apps/rt/src/main.rs` — `read_stdin_input`, `emit_outcome`, `hook_specific_output`
- `apps/rt/src/protocol.rs` — `encode_outcome`, `EncodedResponse` (alternative encoding path)
- `apps/rt/src/registry.rs` — `ToolMatch`, `Module`, `Registry`, `mode_for`

## References

See `apps/rt/.claude/skills/rt-fail-open-dispatch-pattern/references/examples.md` for verbatim code extracts.
