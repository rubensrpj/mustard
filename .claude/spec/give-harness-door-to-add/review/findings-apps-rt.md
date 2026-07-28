## Verdict — REJECTED (1 critical) — SECOND round of the same defect, one column over

Commands: `cargo build --workspace` green; `cargo test --workspace` green on re-runs; the three named AC tests each `ok. 1 passed` in both suites; `cargo clippy -p mustard-rt --all-targets` no `error:`; both `target/debug/mustard-rt.exe` AND the INSTALLED `~/.cargo/bin/mustard-rt.exe` driven against two throwaway git repos. Tree clean, no worktree leaked.

**T1 — PASS in production.** `run ac-add --spec probe --ac AC-9 --command "cd outside"` -> ok:true, proof red, criterion written ABOVE the trailing build criterion, ledger `additions` written, no timestamp on stdout. Four registrations present.

**T2 — PASS.** `criterion_not_proven`, `duplicate_criterion` (including an id only a wave artefact carries — prior MINOR fixed and tested), blank reason/statement, unknown spec; each writes nothing, exit 1.

**T3 (code) — PASS.** `block_end` computed before the `Command:` lookup; test two-sided. Prior MINOR fixed: `prune_empty_parents` confirmed live.

**T4 — CRITICAL.** The command-side decline is real and reproduces the previous probe (`findstr vacuous_marker code.rs` -> `evidence-removed`, `removal_exit:null`, survivor still `survived:1`, exit 2). The mechanism is not inert.

But `remove_one` matches ONLY the command (`ac_negative_check.rs:902`), never `record.expect` — while `qa_run::execute_ac` grades with BOTH. A criterion whose evidence lives in the `Expect:` regex therefore has its red manufactured by the strip and booked as proof. Reproduced end to end:

    spec:  Command: `type lib.txt`   Expect: `beta_marker`   (work appends beta_marker to lib.txt)
    proof red -> confirm green -> removal --from <base>:
      "removed_red": 1, "survived": 0, "evidence_removed": 0
      AC-1: "verdict": "proven", "removal": "red"     EXIT=0

`beta_marker` IS in `removed_text` — the information is in hand one line away and is not consulted. This falsifies three shipped statements at once:
- `ac_negative_check.rs:82-84` — "RED with the criterion's own evidence still intact is a red the behaviour earned"
- `plugin/refs/spec/resume-loop.md:130` — "what it never does is certify a criterion it could not have failed"
- the amended AC-3 itself — "one whose own evidence the strip took away is DECLINED by name instead of being booked as a proven red". Here it was booked.

The AC-3 test cannot catch this: it injects a synthetic `removed_text` and gives no criterion an `Expect:` at all.

**MAJOR — T3 ships named by no criterion.** Nothing runs `a_criterion_without_a_command_line_still_loses_its_whole_statement_block`, so the close would report green having verified nothing about the orphan fix — the exact shape this spec exists to remove. It can no longer be added through the new door either: `ac-add` demands a RED proof and that test is green now.

**MINOR — pre-existing flake outside apps/rt.** `mustard-core` `bench_scan_200_files_under_100ms` failed 1 run in 4 (101.3ms vs 100ms limit), making `cargo test --workspace` non-deterministic.

**MINOR** — the "instale tudo" change request is verified done (the installed binary reproduces the shipped behaviour) but is named by no criterion; an install assertion is machine-local and not reviewable in CI.

Molds: none apply. Guards: all respected.

### What the fix loop must do — LAST of the two allowed rounds
1. Match `taken_away_word` against the criterion's `Expect:` regex as well as its command, OR narrow the shipped claims (AC-3, `ac_negative_check.rs:82-84`, `resume-loop.md:130`) to say the decline covers the command only. Whichever is chosen, the words and the code must agree.
2. Extend the AC-3 test with a criterion whose evidence lives SOLELY in `Expect:`.
3. Name T3 with a criterion. It needs a red — e.g. prove it against the tree with the `ac_amend.rs` rewrite reverted.
