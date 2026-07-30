---
id: wave.close-eleven-harness-defects-found.3-binding
---

# wave-3-binding

## Summary

The record reaches whoever reads it: the session binding lands under the session the hooks resolve, the boundary gate names the boundary it checked, and the work branch record reconciles with the branch actually active.

## Network

- Parent: [[spec.close-eleven-harness-defects-found]]

## Tasks

- [ ] emit_pipeline and context: the session-to-spec binding is written under the session the hooks actually read. Today emit-pipeline run from the CLI carries no harness session id, so the marker lands under a placeholder directory and every gate keyed on it falls back to whichever spec is newest — reproduced live, to the point of blocking an unrelated edit.
- [ ] boundary_gate: the warning names the boundary it actually checked. When the resolved file is a wave spec, say that wave, not the parent slug — the current message sends the author to a section that already lists the file.
- [ ] boundary_gate: a path declared without backticks contributes nothing to the allowed set, and a MIXED spec is the dangerous shape — the backticked entries make the set non-empty and every bare-declared file then warns.
- [ ] work_branch_gate: pre-check the dirty tree the way the other door already does, and after the attempt reconcile the recorded branch with the branch actually active, naming both in the warning. Today the marker is CLEARED on failure, destroying the intent.

## Files

- `apps/rt/src/commands/event/emit_pipeline.rs`
- `apps/rt/src/shared/context.rs`
- `apps/rt/src/hooks/write/boundary_gate.rs`
- `apps/rt/src/hooks/write/work_branch_gate.rs`
