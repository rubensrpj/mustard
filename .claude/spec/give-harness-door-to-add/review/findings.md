## Verdict — REJECTED (1 critical)

Commands run: `cargo build --workspace` (green, 20.6s), `cargo test --workspace` (**4432 passed, 0 failed**), `cargo clippy --workspace --all-targets` (warnings only, no `error:`), plus the real binary driven against three throwaway git trees. Repo working tree unchanged, no worktree leaked.

**T1 — PASS.** Verified in production, not only in unit tests: `run ac-add --spec probe-spec --ac AC-9 ...` wrote `spec.md`, `wave-plan.md` AND `wave-1-rt/spec.md`, left `qa/report.md` alone, and placed AC-9 ABOVE the trailing build criterion. Ledger got `additions[0]` with `at`/`reason`/`wrote`; stdout carries no timestamp.

**T2 — PASS.** `--command "cd ."` -> `error: criterion_not_proven`, `written: []`, no ledger created, exit 1. Duplicate id -> `duplicate_criterion`, exit 1, remedy points at `ac-amend`. Blank reason/statement/unknown spec likewise.

**T3 — PASS.** `block_end` is computed from `ends_block` BEFORE the `Command:` lookup and the `let Some(k) ... else { continue }` moved below the consumption loop (`ac_amend.rs:395-419`), so a command-less criterion loses its whole block; the test is two-sided.

**Guards — PASS.** `ac-add` has all four registrations: enum variant + `dispatch()` arm, `tests/run_command_surface.rs:29`, and a real caller in `plugin/refs/spec/resume-loop.md:117`. No `{role}-pattern` mold applies.

**T4 — CRITICAL. The third transition does not catch either shape it claims to catch.**

The plumbing works: `--removal --from <base>` cut the scratch worktree, reported `taken_away`, drove a behaviour-tied criterion to `removal: "red"`, and exited 2 on a survivor. But the stated capability is false, and it was REPRODUCED:

`ac_negative_check.rs:45` and `resume-loop.md:122` both claim the pass separates behaviour from "the term in a comment, a file that exists and is never called." Both of those live INSIDE the file set `work_removed::declared_paths` strips. The strip deletes the comment along with the code, the command goes red, and the vacuous criterion is CLEARED. Probe: work adds `code.rs` containing only `// TODO: vacuous_marker`, criterion is `findstr vacuous_marker code.rs` — textbook vacuous — and the pass reports `ok: true, removed_red: 1, survived: 0, verdict: proven`.

AC-3's own statement — "a command that is satisfied by a comment rather than by the behaviour is reported as verifying nothing" — is false as shipped. The test that proves AC-3 (`ac_negative_check.rs:1455`) dodges it: its "dragged-along" criterion is a directory present on BOTH trees, i.e. deliberately outside the strip set. The removal only ever detects a criterion pointing at a subsystem the waves never touched.

Worse for this project's own AC convention: every AC here is `cargo test -p mustard-rt <name>` with `Expect: [1-9][0-9]* passed`. The strip deletes the test file, the filter matches 0 tests, the Expect fails, and the criterion goes red UNCONDITIONALLY — the pass cannot fail for the dominant AC shape.

**MINOR — `work_removed.rs:171`** `strip_one` deletes the file but leaves the emptied parent directory, so a directory-existence criterion falsely reports `Survived` (reproduced with `cd behaviour`).

**MINOR — `ac_add.rs:347`** duplicate detection reads only the root `spec.md`, so an id already present in a wave artefact but absent from the root would be inserted twice.

### What the fix loop must do
The removal pass must distinguish "the behaviour is gone" from "the criterion's own evidence file is gone". Stripping the file that the criterion's command reads is what makes the pass unfalsifiable. Whatever the mechanism, the shipped claim in `ac_negative_check.rs:45` and `resume-loop.md:122` must either become true or be rewritten to say what the pass actually separates.
