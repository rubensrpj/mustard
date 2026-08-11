---
id: wave.harness-obstructs-its-own-work.3-prose
---

# wave-3-prose

## Summary

The diagnosis rides into the spec instead of being retyped, and the hygiene question fires on a real collision instead of always.

## Network

- Parent: [[spec.harness-obstructs-its-own-work]]
- Depends on: [[wave.harness-obstructs-its-own-work.2-gate]]

## Tasks

- [ ] In `/bugfix`, make DIAGNOSE's output an INPUT to the spec: after the root cause is located, assemble the conversation material (definitions / decisions with their reason / findings with `file:line`) into `.claude/.cache/spec-material.json` and pass `--material` to `spec-draft`, exactly as `/feature` §2.2 already does. The channel already exists (spec/cli.rs:164) — this flow simply never used it.
- [ ] In the same flow, name `.claude/scratch/` as where DIAGNOSE writes runnable evidence, and carry the compile limit wave 2 documented.
- [ ] Add the weight rule the field report asked for: a bugfix whose root cause is already demonstrated drafts a minimal spec — context, acceptance criteria, boundaries — without the discovery sections it no longer needs.
- [ ] Make the hygiene question conditional (spec-hygiene.md:12): ask only when the new intent OVERLAPS the active spec, or when it was not explicitly requested in the same message. Otherwise record one line — `spec {name} remains parked` — and proceed. State the reason in the ref: a protocol whose steps are routinely skipped teaches the reader to judge every step case by case.
- [ ] Add both prose ratchets to the existing suite, in the shape the file already uses (`*_prose_teaches_*`).

## Files

- `plugin/commands/bugfix.md`
- `plugin/refs/feature/spec-hygiene.md`
- `apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs`
