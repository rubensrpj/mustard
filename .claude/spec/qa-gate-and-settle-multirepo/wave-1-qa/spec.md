---
id: wave.qa-gate-and-settle-multirepo.1-qa
---

# wave-1-qa

## Summary

Stop the QA gate from reporting success over criteria it never ran: drain the child's pipes, tell a timeout apart from a benign skip, and never record a verdict a self-invoked run did not earn.

## Network

- Parent: [[spec.qa-gate-and-settle-multirepo]]

## Tasks

- [ ] Extract the working precedent in `apps/rt/src/commands/pipeline/verify_pipeline.rs` (lines ~385-421: spawn with piped stdout/stderr, drain BOTH on dedicated threads, then poll `try_wait` against a deadline) into ONE shared helper at `apps/rt/src/shared/proc.rs`, registered in `shared/mod.rs`. It must expose: spawn a shell command in a cwd, drain concurrently, wait with a deadline, and return a typed outcome distinguishing `Exited{status, stdout, stderr}` from `TimedOut` and from `SpawnFailed`. No new flag, no env knob.
- [ ] Rewrite `qa_run/runner.rs::run_ac_command` on top of that helper, deleting its own spawn/poll block. Keep every existing behaviour byte-for-byte: the self-invocation guard, `rewrite_self_invoked_cargo`, the `Expect:` evidence gate, the bounded stderr excerpt, and the combined stderr-then-stdout haystack.
- [ ] Make `verify_pipeline.rs` call the shared helper too, deleting its private copy. Two implementations of the same fix are how the sibling drifted out of it in the first place.
- [ ] A criterion killed by the deadline must report `status: "timeout"` — a class of its own, never `skip`. `skip` keeps its meaning: the criterion could not be attempted at all (self-invocation, command not found, invalid Expect pattern).
- [ ] In `qa_run/mod.rs`, the overall verdict must NOT be `pass` when any criterion timed out. Keep today's tolerance for genuine `skip` (the flow documents skip as warn-and-allow) and keep `fail` dominant. Render the new class in `qa/report.md` and in the JSON.
- [ ] When a run is self-invoked AND every criterion came back skipped, suppress the `qa.result` emission entirely (still print the JSON report and still exit as today). Reason: `qa_result_passed` takes the LAST verdict and the close gate defaults to strict, so a run that verified nothing currently invalidates a real external pass and blocks the close. Do not touch the gate itself.
- [ ] Tests: a criterion printing far more than the pipe buffer (~64 KB) completes and is judged by its exit code (this is the regression that must never come back); a criterion that exceeds its deadline reports `timeout` and drags the overall verdict off `pass`; a self-invoked all-skipped run writes no `qa.result` event while a normal run still does; and the existing `verify_pipeline` behaviour is unchanged after the extraction.

## Files

- `apps/rt/src/shared/proc.rs`
- `apps/rt/src/shared/mod.rs`
- `apps/rt/src/commands/review/qa_run/runner.rs`
- `apps/rt/src/commands/review/qa_run/mod.rs`
- `apps/rt/src/commands/pipeline/verify_pipeline.rs`
