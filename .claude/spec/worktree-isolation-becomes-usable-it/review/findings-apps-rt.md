## Verdict: REJECTED — 1 critical (re-review after the withdrawal fix loop)

All 11 ACs and all controls green; full `cargo test -p mustard-rt` → 3965 passed, 0 failed; clippy 0 errors.

**Prior criticals genuinely closed.** `fn carry_environment`, `fn link_dir`, `WorktreeConfig`, `normalise_relpaths` no longer exist outside `#[cfg(test)]`. `cut_pending_work_branch` — the door `spec-draft` opens first — now takes `busy_checkout` and answers `CutOutcome::Refused`; AC-11 drives that function directly, not the gate. The `Deny` now renders through `translate("workbranch.busy.refusal")`, closing the prior i18n nit.

## CRITICAL — the acting collector deletes unsaved work when its safety probe cannot answer

`apps/rt/src/commands/maint/worktree_gc.rs:411` guards removal with `dirty_paths(&wt)`, and `apps/rt/src/commands/work_unit_open.rs:222` returns `Vec::new()` on **any** git failure. So "could not measure" reads as "holds nothing" — the exact inversion this wave's own recorded decision forbids for `process_liveness` ("treat `None` as not measured, not allowed"), left in place one line later for the work probe. Line 498 is what makes it bite: `session_start_probe` now runs `apply = true` at **every** SessionStart (it was `apply = false` before this wave), so the latent opt-in path became automatic.

Reproduced end to end in a scratch repo — a stale directory under `.claude/worktrees/` (not a registered worktree, the shape the field already has) holding two unsaved files:

```
main checkout status: (clean)
git -C .claude/worktrees/bright-running-fox status --porcelain → (empty — it answered about the MAIN repo)
mustard-rt run worktree-gc --apply → "removed": [".claude\\worktrees\\bright-running-fox"]
precious.txt still there? NO — DELETED
```

`git status` inside a non-worktree directory resolves to the enclosing repo, so the candidate's protection depends on an unrelated fact (whether the main checkout has non-`.claude/` dirt). Not fail-open-to-safe — fail-open-to-arbitrary. Confirmed live on this repo: `run worktree-gc` reports `.claude/worktrees/recursing-benz-063389` (age 22d, no `.git` file, no admin record) as reason `"dry-run"` — i.e. the next SessionStart removes it, having judged it by the main checkout. It is empty today; the guard is not. AC-5 does not cover this class: its fixture is a real registered worktree, where git can answer.

INDEPENDENTLY CONFIRMED BY THE ORCHESTRATOR (2026-08-12), and the mechanism is sharper than fail-open-on-failure. The probe does not merely fail — it is BLINDED BY DESIGN for exactly the tree the collector walks. `dirty_paths` deliberately drops every path under `.claude/` (`work_unit_open.rs:238`), and a directory under `.claude/worktrees/` reports its own contents under that prefix:

```
scratch repo, .claude/worktrees/pasta-antiga/precioso.txt holding unsaved work
git -C .claude/worktrees/pasta-antiga status --porcelain  →  "?? .claude/"
after the .claude/ carve-out at work_unit_open.rs:238      →  (empty)  →  read as CLEAN  →  eligible for removal
```

So the carve-out that makes `dirty_paths` correct for its ORIGINAL caller (a worktree cut decision, where `.claude/` is redirected state and not code) is precisely what blinds it for this one. The judgement belongs in the consumer: a deleting caller must treat "not measured" as "not allowed", and must not inherit a carve-out written for a different question.

## Non-blocking

- **major** — the field leak the spec cites as its own evidence is still not collected. `mustard-removal-mustard-31860` (PID dead) is now within reach but kept as "holds uncommitted work" because it predates `record_the_strip` (`work_removed.rs:265`). Needs one manual `git worktree remove --force`.
- **minor** — spec AC-3 declares Control `work_unit_open`, but its test lives in `work_branch_gate.rs:1302`.
- **minor** — the AC-9 ratchet's `"mklink /J"` literal (`plugin_prose_matches_shipped_behaviour.rs:659`) cannot match the arg-array spelling actually used, so that clause is inert.
- **minor** — commit `1a15d4bd` bundles ~1.2k lines for a different spec onto this unit's branch.

<VERDICT>{"verdict":"rejected","critical":1,"findings":[{"severity":"critical","location":"apps/rt/src/commands/maint/worktree_gc.rs:411","summary":"the now-acting SessionStart collector treats an unmeasurable dirty-probe as 'holds nothing'; worse, dirty_paths drops every .claude/ path by design, which is exactly where the collector's candidates live, so their contents are invisible to the guard"},{"severity":"major","location":"apps/rt/src/commands/review/work_removed.rs:265","summary":"the live leak mustard-removal-mustard-31860 is within reach but still kept as holding work"},{"severity":"minor","location":".claude/spec/worktree-isolation-becomes-usable-it/spec.md:47","summary":"AC-3 declares the wrong Control module"},{"severity":"minor","location":"apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs:659","summary":"the mklink ratchet clause is inert"},{"severity":"minor","location":"apps/rt/src/hooks/write/boundary_gate.rs:1","summary":"commit 1a15d4bd bundles a different spec onto this branch"}]}</VERDICT>
