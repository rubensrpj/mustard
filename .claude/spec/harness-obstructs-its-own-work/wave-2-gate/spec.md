---
id: wave.harness-obstructs-its-own-work.2-gate
---

# wave-2-gate

## Summary

Diagnosis may produce runnable evidence without leaving the protected base, and the harness stops dirtying its own tree.

## Network

- Parent: [[spec.harness-obstructs-its-own-work]]

## Tasks

- [ ] Exempt `.claude/scratch/` from branch protection in `work_branch_gate`, beside the existing `.claude/plans/` carve-out (work_branch_gate.rs:286-306): a write there is allowed on a bare integration base, cuts NO branch, and leaves any pending work-unit marker intact for the first real edit — the same contract the plan-file exemption already has.
- [ ] State the limit in the module doc: the carve-out serves scratch evidence a runner can execute in place (scripts, data, `mustard-rt run …` probes). Evidence that must COMPILE inside a crate cannot live there — cargo does not compile files under `.claude/` — and for that case the honest path is opening the unit, which wave 3 makes cheap.
- [ ] Add `scratch/` to the seeded `.claude/.gitignore` so the sanctioned scratch path is ignored by construction.
- [ ] Add the harness artefacts the runtime writes into `.claude/` and nothing ignores: `feature-digest.json`, and each spec's `qa-report.json` and `qa/` directory. These are the files found blocking the exit ritual in the field.
- [ ] Do not touch `.claude/spec/`'s tracked content: a spec belongs to its unit and stays versioned — only the QA sidecars and the digest are runtime output.

## Files

- `apps/rt/src/hooks/write/work_branch_gate.rs`
- `packages/core/templates/.gitignore`
- `packages/core/tests/seeded_ignore.rs`
