## Verdict — APPROVED (0 critical)

The round-2 CRITICAL is genuinely fixed, verified in production, not only in unit tests.

**T4 / previous CRITICAL — FIXED.** `taken_away_word` now takes command AND `Expect:` as one argument (`work_removed.rs:119`); the single call site passes both (`ac_negative_check.rs:917`). Reproduced the exact defect shape end to end in a throwaway git repo (`Command: type lib.txt`, `Expect: beta_marker`, work appends the marker, `diff.md` declares `lib.txt`): `removed_red: 0, survived: 0, evidence_removed: 1`, `"removal": "evidence-removed"`, `removal_exit: null`, reason names `beta_marker`. Previously this was `removed_red: 1 / "proven"`. Same result from the INSTALLED `~/.cargo/bin/mustard-rt.exe`.

**Not over-declining (the other direction).** Second production repro, criterion pointing at a file the work never declared: `survived: 1, ok: false, EXIT=2`. The falsifying half still fires, so the decline did not swallow the pass.

**Two-sidedness of the new AC-3 case is real.** AC-4's command names nothing in `removed_text`, and the stripped tree holds the file WITHOUT the marker — with the expect half ignored it would run, exit 0, miss `Expect:` and land as a proven red, failing the `EvidenceRemoved` assertion.

**AC tests — each `ok. 1 passed`**: `ac_add_lands_only_after_taking_the_proof`, `ac_add_refuses_a_criterion_that_cannot_fail`, `removal_refuses_a_survivor_and_declines_what_it_cannot_judge`, `a_criterion_without_a_command_line_still_loses_its_whole_statement_block` (two-sided). `cargo build --workspace` green; `cargo test --workspace` EXIT=0, 4434 passed, no FAILED (the 100 ms bench flake no longer reproduces).

**T3 named by a criterion — round-2 MAJOR CLEARED.** AC-5 was added through the door itself, ledger `additions[0]`, proof `red` with `exit: 101` and a `stderr_excerpt` showing the test COMPILED and FAILED — so the red came from reverting the fix, not from a filter matching zero tests. That distinction is what makes it a real proof.

**Guards — all respected.** Clippy clean (unwrap/expect deny), no new `run` subcommand this round, `ac-add`'s four registrations intact, observers/`main.rs`/hook degradation untouched, new stdout fields carry no timestamps, no worktree leaked. **Molds — none apply.**

Non-blocking findings:
- MINOR `store.rs:271` — bench limit relaxed 100ms -> 1s; the flake is gone and the doc now states the budget only catches order-of-magnitude regressions, but it remains a wall-clock assertion (outside apps/rt).
- MINOR `ac_negative_check.rs:933` — `stderr_excerpt` carries absolute machine paths and elapsed times into `run` stdout and the versioned `ac-proof.json`; pre-existing (shipped in an earlier merged spec) but now also flows through the new `ac-add` report, brushing the byte-stability guard.
- MINOR `work_removed.rs:124` — words from a standard `Expect:` regex (`passed`, `1-9`, `0-9`) participate in the match, so a strip removing those words anywhere can decline a criterion that should have been judged. Safe direction and visible via `evidence_removed`, but it can silence a survivor finding.
- MINOR — the installed plugin cache carries neither `ac-add` nor `ac-negative-check` prose, so the prose half of "install everything" is not live. Pre-existing/systemic: the cache is a frozen photocopy refreshed by a release plus `claude plugin update`.
- MINOR `ac-proof.json` — every criterion still shows `confirmation: "not-taken"`; the green half is QA's step and is not recorded yet.
