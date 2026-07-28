---
id: wave.make-harness-stop-asserting-what.5-checklist
---

# wave-5-checklist

## Summary

Work dropped on purpose is recorded as a decision, so it stops reading as work someone forgot.

## Network

- Parent: [[spec.make-harness-stop-asserting-what]]
- Depends on: [[wave.make-harness-stop-asserting-what.1-proof]]

## Tasks

- [ ] Give a checklist item a third position besides open and done: dropped, which cannot be written without a stated reason.
- [ ] Give the wave lifecycle the matching state, so deliberately abandoned work is not indistinguishable from work never started — honouring the serde contract other crates render against.
- [ ] Keep the marker's refusal honest: it must still refuse to mark what it cannot find, and must never turn a dropped item back into a pending one.

## Files

- `apps/rt/src/commands/checklist/mark_checklist_item.rs`
- `packages/core/src/domain/model/view/wave.rs`
