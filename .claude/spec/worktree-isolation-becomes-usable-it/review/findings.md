All ten ACs verified by running them, plus the full suite, clippy, and a field check of the real leaked worktree.

## Per-AC result (commands run, real output)

| AC | Command | Result |
|---|---|---|
| 1 | `cargo test -p mustard-rt a_declared_carry_path_lands_in_a_fresh_worktree` | `ok. 1 passed` (×2 targets) — PASS |
| 2 | `…a_declared_link_path_reaches_the_main_checkout` | `ok. 1 passed` — PASS (junction fallback works unelevated here) |
| 3 | `…what_did_not_travel_is_named_and_never_aborts` | `ok. 1 passed` — PASS |
| 4 | `…an_orphan_worktree_is_collected_without_waiting_for_age` | `ok. 1 passed` — PASS |
| 5 | `…the_acting_collector_still_refuses_a_worktree_holding_work` | `ok. 1 passed` — PASS |
| 6 | `…an_abandoned_removal_worktree_is_within_reach_and_collected` | `ok. 1 passed` — PASS (see MAJOR-1) |
| 7 | `…a_second_unit_is_isolated_instead_of_taking_the_checkout` | `ok. 1 passed` — PASS |
| 8 | `…the_first_units_uncommitted_work_stays_where_it_was` | `ok. 1 passed` — PASS |
| 9 | `…worktree_prose_teaches_the_declared_environment` | `ok. 1 passed` — PASS |
| 10 | `cargo build --workspace` | 0 errors, 2 warnings (both pre-existing, in untouched `feature.rs` / `branch_state.rs`) — PASS |

Regression control: `cargo test -p mustard-rt` → `1934 passed; 0 failed` + `1939 passed; 0 failed` + all integration suites green. `cargo test -p mustard-core` → `608 passed; 0 failed`. `cargo clippy --workspace --all-targets` → no errors.

Guards (`apps/rt/CLAUDE.md`) and `rt-gate-pattern` mold: no violation. The new step 2.5 stays inside `evaluate`, expresses itself as `Verdict::Deny`, never `Err`, degrades to the in-place cut when `hook_create` fails; no `unwrap`/`expect` outside `#[cfg(test)]`; tests live at the bottom of each file. `hook_create` (not `open_at`) is the entry used, per the wave-3 memory.

## Findings (none blocking)

**MAJOR-1 — the field leak that motivated the spec is still not collected.** Verified live: `git worktree list` shows `C:/Users/ruben/AppData/Local/Temp/mustard-removal-mustard-31860`; `tasklist /FI "PID eq 31860"` → no such process; `git -C … status --porcelain` → 7 entries. `gc` (worktree_gc.rs:411) classifies it `holds uncommitted work` and keeps it forever. The fix works by making `work_removed::build` commit the strip (work_removed.rs:265), so only trees abandoned *after* that commit are clean enough to reap; a pass killed between `worktree add` (work_removed.rs:212) and `record_the_strip` still leaks permanently. AC-6's test builds an unstripped clean tree, which is not what an interrupted strip leaves. Non-blocking: the "never remove a worktree holding work" refusal is an explicit Non-Goal of this spec, and the window is now seconds instead of the whole pass. The existing directory needs one manual `git worktree remove --force`.

**MINOR-2 — new refusal text bypasses the i18n catalogue.** `apps/rt/src/hooks/write/work_branch_gate.rs:414` hardcodes pt-BR in `format!`, while the sibling `Verdict::Warn` at :490 renders `translate("workbranch.reconcile.warn", lang)` with `lang` already in scope. Consistent with two pre-existing hardcoded `Deny`s in the same file, so it is debt continued rather than introduced.

**MINOR-3 — unrelated work rides this branch.** Commit `1a15d4bd` bundles a separate tactical fix (`boundary_gate.rs`, `subagent_inject.rs`, `wave_done.rs`, `dispatch_plan.rs`, ~1.2k lines) under its own spec dir `.claude/spec/2026-08-12-o-registro-por-onda/`. No AC of this spec covers it; it is green in the suite but unreviewed by this gate.

**MINOR-4 — the acting collector's age fallback now deletes without any owner probe.** `session_start_probe` runs `gc(..., apply = true)` (worktree_gc.rs:289) at every session start; a platform worktree under `.claude/worktrees/` (e.g. `recursing-benz-063389`) yields `Ownership::Unknown`, so a clean one older than 7 days is removed with `git worktree remove --force`. Pre-existing rule, newly effective — correct per the spec's decision, worth knowing operationally.

Change requests "ar" and "Segue" carry no content to verify.

<VERDICT>{"verdict":"approved","critical":0,"findings":[{"severity":"major","location":"apps/rt/src/commands/review/work_removed.rs:265","summary":"a scratch worktree abandoned before record_the_strip stays dirty and is never collected — the live mustard-removal-mustard-31860 leak (dead PID, 7 dirty paths) is still kept as 'holds uncommitted work'"},{"severity":"minor","location":"apps/rt/src/hooks/write/work_branch_gate.rs:414","summary":"new Deny reason hardcodes pt-BR instead of translate(), while the sibling Warn at :490 uses the i18n catalogue"},{"severity":"minor","location":"apps/rt/src/hooks/write/boundary_gate.rs:1","summary":"commit 1a15d4bd bundles an unrelated tactical fix (~1.2k lines across 4 pipeline/hook files) into this unit's branch, covered by no AC of this spec"},{"severity":"minor","location":"apps/rt/src/commands/maint/worktree_gc.rs:289","summary":"SessionStart now removes with --force via the age fallback for platform worktrees whose ownership cannot be probed"}]}</VERDICT>
