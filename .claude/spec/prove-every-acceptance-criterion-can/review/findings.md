# Review — apps/rt — verdict: approved, 0 critical

Every AC command was independently re-run in the worktree: AC-1..AC-18 green.
Full suite: lib 1714 passed, bin 1719 passed, template_parity 3 passed.
Clippy --workspace --all-targets: warnings only, zero errors (the
unwrap_used/expect_used deny holds).

Driven live against a temp project with the built binary, not taken on trust:
- `run ac-negative-check --spec demo` → exit 2, ledger written, AC-1 proven/red,
  AC-2 unproven/green, AC-3 exempt.
- `run ac-amend --ac AC-2 --command "cd another-no-such-dir" --reason ...` →
  exit 0, rewrote root spec.md + wave-plan.md + wave-1-rt/spec.md.
- With AC-1's command hand-edited, `MUSTARD_APPROVAL_MODE=off run approve-spec`
  → exit 1, refusal names "AC-1 — the proof was NEVER TAKEN" and states the
  proof precondition is UNCONDITIONAL.

Guards: both new commands carry all FOUR registrations; RUNTIME_WHITELIST was
not touched and the reverse ratchet passes. Mold contract respected.

## MAJOR — the two change-request doc edits are named by no acceptance criterion

AC-10 asserts only `plugin/refs/spec/resume-loop.md`. No test anywhere reads
`plugin/pipeline-config.md` or `plugin/refs/feature/glossary-grill.md`, so both
could be reverted and the whole suite would stay green. This is precisely the
failure this spec's own resume-loop text names: a request that is implemented
but unnamed by any criterion makes the gate report green without ever verifying
it. Non-blocking — the work was done, only its proof is missing.

## MINOR — flaky test under parallel load

`qa_run/runner.rs::tests::ac_command_with_quotes_and_parens_runs_verbatim`
failed once during a full-workspace run, then passed isolated and on rerun.
`runner.rs` is untouched by this diff — pre-existing.

## MINOR — a pipeline tail still escapes the weak verdict

`is_weak_ac_command` judges a pipeline by its parts, so `rg -q X src | wc -l`
reads strong because `wc` is not in the asserts-nothing set, even though a
pipeline's exit status comes from its tail. The linter is WARN by design and
ac-negative-check catches this at runtime, so the escape is closed one layer
down.

## MINOR — stderr_excerpt on stdout carries locale-dependent OS text

`ac-negative-check` / `ac-amend` publish `stderr_excerpt`; locale-dependent OS
text was observed live, and a cargo criterion would carry machine paths and
timings. `plan-materialize` deliberately strips it from its own slot and
`qa_run::criteria_json` sets the precedent, so no snapshot is at risk today.
