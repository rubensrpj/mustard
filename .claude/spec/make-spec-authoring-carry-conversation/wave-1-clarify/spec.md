---
id: wave.make-spec-authoring-carry-conversation.1-clarify
---

# wave-1-clarify

## Summary

Give the clarification gate a state of the world in which it fails: the marker records WHAT was settled, and approval refuses one that recorded nothing.

## Network

- Parent: [[spec.make-spec-authoring-carry-conversation]]

## Tasks

- [ ] Today `grill-capture --finalize` mints the marker unconditionally — its own doc states it needs no term, so a complete-glossary spec is not deadlocked. That fix removed a deadlock and created a decoration: the orchestrator mints the marker seconds before requesting the approval the marker unlocks. Keep the deadlock fixed; kill the decoration. The finalize must RECORD, not merely mint.
- [ ] Write substance into the marker: which grill ran, its verdict, the terms captured, or — the case that replaces the old escape hatch — an explicit stated reason why no grill applied. A complete glossary is a legitimate reason. Silence is not. Keep the file format simple and byte-stable (the current `key=value` shape is fine); this is a record, not a database.
- [ ] Accept the decline as first-class. `--finalize --reason "<why>"` is the honest path when a grill would not pay off. It must be a stated sentence, not a flag: the operator or the orchestrator says why, and that sentence is what a later reader sees.
- [ ] In `approve_spec.rs`, the gate currently checks the marker EXISTS. Make it check the marker CARRIES something. An empty or substance-free marker refuses the approval, and the refusal names which grill to run or tells the caller to state a reason. This is the whole point of the wave: after it, there is an input for which the gate says no.
- [ ] Fail-closed here, deliberately, and say so in the code: this guards a verdict. Every other gate in this crate fails open, so a reader will assume this one does too unless the comment states why it must not. A missing marker already refuses today; a hollow one must refuse for the same reason.
- [ ] Test `clarified_marker_records_what_was_settled`: finalize with captured terms and assert the marker names them; finalize with a stated reason and assert the reason survives verbatim. Test `approve_refuses_a_marker_that_recorded_nothing`: a marker holding only the spec name must make approval return `ok:false` with a reason naming the remedy, and the approval events must NOT be emitted.

## Files

- `apps/rt/src/commands/grill_capture.rs`
- `apps/rt/src/commands/spec/approve_spec.rs`
