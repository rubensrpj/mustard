## Verdict: REJECTED — 3 critical

**Build & tests (verified, not taken on trust)**
- AC-9 `cargo build --workspace` -> green.
- AC-1...AC-8: each named test run individually -> `test result: ok. 1 passed; 0 failed` (non-zero, satisfies `[1-9][0-9]*`). All eight tests are genuinely two-sided (red-then-green fixtures, controls that must NOT fire) — not tautologies.
- `cargo clippy --workspace --all-targets` -> **0 errors**, so the `unwrap_used`/`expect_used` deny guard holds.
- Full workspace suite green on a clean rerun (mustard_rt 1746 + 1751, mustard_core 598, all integration bins 0 failed).

**Guards + molds:** no violation found. `close_gate.rs` stays fail-open with the mode cascade (rt-gate-pattern); `post_edit.rs` `observe()` still returns `()` and returns no verdict (rt-observer-pattern); `--confirm` is a flag on an existing subcommand, so the four-registration rule does not apply; `run` output stayed ordered JSON.

**The blocking findings — all three mid-pipeline change requests were silently dropped.** `git diff --name-only 806da3b4 HEAD -- plugin/` returns exactly two files: `refs/agent-prompt/agent-prompt.md` and `refs/feature/full-plan.md`. None of the three files the operator named was touched.

1. **CRITICAL — CR-1 dropped; the confirm pass ships inert.** `plugin/refs/spec/resume-loop.md` and `plugin/pipeline-config.md` are unmodified. Worse, `ac_negative_check::confirm` has **no production caller**: grep finds it only at `ac_amend.rs:904`, inside `#[cfg(test)] mod tests` (module starts line 670). `plan_materialize.rs:302` still calls `check` (the red pass). So the entire second half of AC-1 is reachable only by a human typing `--confirm`, and nothing anywhere tells anyone it exists. The unit test proves the function works; it does not prove the harness ever takes the confirmation — which is this spec's own thesis about code presence vs. effectiveness.

2. **CRITICAL — `plugin/refs/spec/resume-loop.md:100` now states a falsehood.** It reads: *"the new command is run against the tree as it is and **must itself come back RED**. A replacement that already passes ... is REFUSED."* AC-2 deliberately changed that (`ac_amend.rs:526`, `predecessor_inexecutable`). The operator-facing prose now contradicts shipped behaviour and will send a reader to abandon the one sanctioned repair path this spec built.

3. **CRITICAL — CR-3 dropped; `neverDispatched` is emitted and undiscoverable.** Field at `resume_bootstrap/mod.rs:127-128`, printed at `:455`. `plugin/refs/spec/resume-loop.md:25` still instructs the orchestrator to read `isWavePlan/totalWaves/currentWave` only; grep for `neverDispatched` across `plugin/` returns nothing.

**Non-blocking**
- MAJOR — CR-2 dropped. `active_specs.rs:1097` prints an `Onde` column, but `plugin/commands/spec.md:25` — a block that section 2 orders printed "literally" — still lists only `#`/`Esc`/`Prog`/Stage/Status.
- MAJOR — none of the three requests got an Acceptance Criterion; `spec.md` still carries AC-1...AC-9 unchanged. `resume-loop.md:92` is the harness's own rule against exactly this: *"a request that is implemented but unnamed by any AC makes the gate report green without ever verifying it."* Here they were not even implemented, so QA reports green over three dropped requests.
- MINOR — one unreproducible failure: while two cargo jobs ran concurrently, the mustard_core lib binary reported `test result: FAILED. 597 passed; 1 failed; 3 ignored`. Five isolated reruns and a clean full-workspace rerun were all `598 passed; 0 failed`. I could not name it; recording rather than dismissing it.
- MINOR — `wave_advance.rs`, `close_gate.rs`, `post_edit.rs` edited outside their waves' `## Files`. Both cascades are correct and necessary, and the deviations are already recorded as accepted decisions.

The eight fixes themselves are well built and honestly tested. What blocks is that three mechanisms this spec shipped — the confirmation pass, `neverDispatched`, and the `Onde` column — reach no reader, and one doc now actively misstates the code. That is the same failure the spec was written to remove.
