---
id: wave.prove-every-acceptance-criterion-can.1-rt
---

# wave-1-rt

## Summary

The negative-test engine: run each criterion against the tree as it is, require it to fail, record the proof — plus the removal of the search-for-absence exemption that let a criterion read green while matching nothing.

## Network

- Parent: [[spec.prove-every-acceptance-criterion-can]]

## Tasks

- [ ] Add `apps/rt/src/commands/review/ac_negative_check.rs`. Core, testable against an explicit project root (never `current_dir()`), mirroring how `analyze_validation::validate` takes `root` as a parameter: read the spec markdown, parse criteria through the SHARED `qa_run::extract_ac_section` + `qa_run::parse_ac_items` (never a second parser), and for each criterion execute its command through the SHARED `qa_run` executor so the deadline, the pipe drain and the `Expect:` regex grading are the ones QA itself uses. Expose whatever minimal `pub(crate)` seam that reuse needs from `qa_run` — do NOT copy the executor.
- [ ] Classify each criterion into exactly two outcomes: PROVEN when the command comes back non-zero (or exits 0 but its declared `Expect:` regex misses), and UNPROVEN for everything else — green, timed out, or a command still carrying an unfilled `<…>` placeholder. Each unproven entry carries a short human reason. The rule is one sentence and must read that way in the module docs: a criterion clears the proof ONLY by failing. NOTHING else may produce a refusal — a gate that refuses for reasons the reader cannot act on teaches the caller to route around it.
- [ ] Exempt the LAST criterion by position, reusing the rule the tautology linter already applies at `analyze_validation.rs:599` — it is the trailing build-green safety net and is green before the work by design. One exemption in the codebase, not two.
- [ ] Write the proof ledger `<spec-dir>/ac-proof.json`: for each criterion its id, the EXACT command string that was run, the declared expect regex, the verdict, the exit code and a bounded stderr excerpt; entries sorted by id so the file is byte-stable. Keep an `amendments` array in the same document (empty here — wave 3 appends to it). A criterion whose recorded command still matches is NOT re-run on a later pass: the ledger is the reason the gate stays stable when the command later starts passing for the honest reason.
- [ ] Publish it as `mustard-rt run ac-negative-check --spec <path-or-slug>`: variant in `ReviewCmd` AND the arm in its `dispatch()` (both in `review/cli.rs`), module registered in `review/mod.rs`. Print one JSON document on stdout, byte-stable and free of timestamps or volatile paths (there are snapshot gates on `run` output). Exit 0 when every non-exempt criterion is proven, 2 when any is unproven — and write the ledger either way, so the proofs already obtained are not lost.
- [ ] Distinguish NEVER TAKEN from TAKEN AND GREEN in every message the engine produces, and expose the distinction in the report. Absence of a proof and a proof that came back green are opposite situations asking for opposite actions (run it, versus rewrite the criterion); collapsing them into one wording is how a caller learns to read a missing artefact as a failure.
- [ ] In `analyze_validation.rs`, DELETE the search-for-absence exemption: `is_absence_search` and its use in `is_weak_ac_command`. Neither spelling is safe — `--files-without-match` exits 0 precisely when the pattern matches nothing, and `-v` exits 0 when any single line fails to match.
- [ ] Close the SECOND escape in the same function, confirmed at `analyze_validation.rs:343`: a command containing `&&`, `||`, `;` or `|` returns NOT-weak immediately, on the reasoning that the author combined steps on purpose. Field evidence shows the shape that walks straight through it — a literal search chained to a step that asserts nothing (`rg -q '<literal>' <path> && echo OK`), which is a presence search wearing a compound coat. Do NOT simply delete the compound exemption: a genuinely combined command (`cargo test -p x foo && ./verify.sh`) must stay strong. Split the command on its operators and judge the PARTS — the whole is weak when every part is weak, where a part that asserts nothing (a bare `echo`, `true`, `:`) counts as weak. Keep the pipeline case conservative if the parts cannot be split reliably; a false WARN costs a sentence, a false 'strong' costs a criterion.
- [ ] Replace the existing `absence_search_is_not_weak` test with `a_search_that_cannot_fail_is_never_exempt`, asserting all three directions: an absence search is now weak, a literal search chained to `echo OK` is weak, and a genuinely combined command with a real assertion is still strong. Correct the module and function docs that state the old rules.

## Files

- `apps/rt/src/commands/review/ac_negative_check.rs`
- `apps/rt/src/commands/review/mod.rs`
- `apps/rt/src/commands/review/cli.rs`
- `apps/rt/src/commands/review/analyze_validation.rs`
