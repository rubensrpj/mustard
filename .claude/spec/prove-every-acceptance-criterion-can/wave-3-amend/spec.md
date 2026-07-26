---
id: wave.prove-every-acceptance-criterion-can.3-amend
---

# wave-3-amend

## Summary

Amending a criterion becomes an operation: it demands the same negative proof from the replacement and keeps the superseded version with the reason it was replaced.

## Network

- Parent: [[spec.prove-every-acceptance-criterion-can]]
- Depends on: [[wave.prove-every-acceptance-criterion-can.1-rt]]

## Tasks

- [ ] Add `apps/rt/src/commands/spec/ac_amend.rs` following the shape `change_request.rs:110` already established here: an options struct, a serializable report struct, a core routine taking an explicit project root so it is testable against a temp tree, and a re-read that REPORTS whether the write landed instead of assuming it.
- [ ] Publish `mustard-rt run ac-amend --spec <slug> --ac <AC-N> --command <new command> [--expect <regex>] [--statement <new text>] --reason <why>`: variant in `SpecCmd` AND the arm in its `dispatch()`, module registered in `spec/mod.rs`. The name is `ac-amend`, never bare `amend` — `amend-finalize` already means the unrelated session-end window.
- [ ] Refuse, writing NOTHING, on: a blank reason, an unknown spec directory, an unknown criterion id, and — the load-bearing one — a replacement command that the wave-1 engine reports as NOT proven. Each refusal names its own reason AND what to do about it in the report. A replacement that already passes proves exactly as little as the original did.
- [ ] On acceptance, rewrite the criterion in EVERY artefact under the spec directory that carries that id on a criterion line — the root `spec.md`, `wave-plan.md` and each `wave-*/spec.md` — because the scaffold is frozen after approval (`wave_scaffold.rs:578`) and a criterion amended only at the root leaves the dispatched agent reading the superseded command. Report the list of files actually rewritten.
- [ ] Append to the `amendments` array of the same `ac-proof.json` the id, the superseded command and expect regex, the new ones, the stated reason and a timestamp; and update that criterion's proof record so the approval door in wave 2 accepts the NEW command. Timestamps live in the ledger, never on stdout.
- [ ] Unit-test both directions against a temp project: a vacuous replacement is refused with nothing written anywhere, and an accepted amendment records the superseded version with its reason and rewrites the criterion in the root spec AND in a planted wave artefact.

## Files

- `apps/rt/src/commands/spec/ac_amend.rs`
- `apps/rt/src/commands/spec/mod.rs`
- `apps/rt/src/commands/spec/cli.rs`
