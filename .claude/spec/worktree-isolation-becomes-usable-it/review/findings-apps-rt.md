## Verdict — REJECTED (1 critical) — fourth review

All 12 ACs green; `cargo test --workspace` → 4809 passed, 0 failed (70 suites); clippy no errors. The third review's critical is genuinely closed: `busy_checkout` (`work_branch.rs:483`) measures with its own `checkout_work` (`:370`), which keeps `.claude/` and answers `Unproven` on a failed probe. Guards and molds hold.

### CRITICAL — the probe counts `.claude/`, but the ignore rules keeping the harness's OWN scratch out of it ship only to NEW projects

`checkout_work` drops the `.claude/` carve-out and its doc (`work_branch.rs:352-362`) justifies that by "the VOLATILE harness state … is gitignored by the seeded `.claude/.gitignore`". The rules that make that sentence true were added to `packages/core/templates/.gitignore:7-17` ONLY. `seed_gitignore` is called from `init.rs:186` with an `overwrite` the operator chooses interactively (`init.rs:148`, "Merge (keep my files)"), so every already-seeded project keeps the old eight-rule file — including **this repository's own `.claude/.gitignore`**, which still lacks `.session/` and is saved only by an unrelated `**/.claude/.session/` line in the ROOT `.gitignore:118`.

Reproduced on a real git tree with the pre-change seed:

```
$ git status --porcelain        # after the gate wrote .claude/.session/<sid>/pending-work-branch
?? .claude/.session/
```

`??` parses → `CheckoutWork::Holds([".claude/.session/"])` → `busy_checkout` → `Deny` / `CutOutcome::Refused`. A checkout CLEAN of anybody's work is refused over the marker the gate itself just wrote, and the operator is told to commit or stash the harness's droppings. This contradicts the shipped prose row ("ANOTHER unit's branch, tree CLEAN | the in-place cut") and defeats the counterweight `a_clean_checkout_lets_the_cut_through`, which passes only because its fixture hand-writes `HARNESS_SCRATCH_IGNORE` (`work_branch.rs:703`) instead of reading the shipped seed. Nothing binds `CLAUDE_GITIGNORE` to the probe: `seeds.rs:54` asserts only `.events/`, and `packages/core/tests/seeded_ignore.rs` does not exist.

INDEPENDENTLY CONFIRMED BY THE ORCHESTRATOR (2026-08-13):
```
.claude/.gitignore of this repo: no .session/ entry (8 rules, ends at spec/*/.blobs/)
root .gitignore:118:             **/.claude/.session/    <- the only thing saving this checkout
```
The design error is delegating a correctness-critical exclusion to per-project configuration that can be stale, absent or hand-edited. The harness knows its own scratch paths; the probe must own that list rather than ask the project.

### Non-blocking

- **major** — the mid-pipeline request approved 2026-08-12 (`fix/`/`feature/`/`hotfix/` branch types, asked with a default per scenario and a selectable base) is implemented nowhere, covered by no AC, and has no follow-up spec. Second round it survives.
- **major** — commit `1a15d4bd` puts ~1.2k lines of spec `2026-08-12-o-registro-por-onda` on this unit's branch, outside `## Boundaries`.
- **major** — `memory/cut-worktree-from-inside-apps-call-wave3.md:3` teaches that `hook_create` runs `carry_environment` — a function this same unit deleted.
- **major** — the field leak `mustard-removal-mustard-31860` is within reach but still kept as holding work; needs one manual removal.
- **minor** — AC-3 declares Control `work_unit_open`; the test lives at `work_branch_gate.rs:1344`.
- **minor** — the AC-9 ratchet literal `"mklink /J"` cannot match the arg-array spelling; that clause is inert.
- **minor** — `work_removed.rs:553` proves the recorded strip left the tree clean with `dirty_paths`, weaker than the `contents` probe the collector applies.

<VERDICT>{"verdict":"rejected","critical":1,"findings":[{"severity":"critical","location":"packages/core/templates/.gitignore:8","summary":"checkout_work counts .claude/ but the ignore rules keeping the harness's own .session/ marker out of git status ship only in the seed template — every already-seeded project refuses a CLEAN second-unit cut over the marker the gate itself wrote; the probe must own its scratch list instead of delegating to per-project config"},{"severity":"major","location":".claude/spec/worktree-isolation-becomes-usable-it/change-log.md:24","summary":"approved branch-naming request has no code, no AC and no follow-up spec"},{"severity":"major","location":"apps/rt/src/hooks/write/boundary_gate.rs:1","summary":"commit 1a15d4bd bundles a different spec onto this unit's branch"},{"severity":"major","location":".claude/spec/worktree-isolation-becomes-usable-it/memory/cut-worktree-from-inside-apps-call-wave3.md:3","summary":"promoted memory teaches hook_create runs carry_environment, deleted by this unit"},{"severity":"major","location":"apps/rt/src/commands/review/work_removed.rs:325","summary":"the field leak cited as the spec's evidence is still kept as holding work"},{"severity":"minor","location":".claude/spec/worktree-isolation-becomes-usable-it/spec.md:47","summary":"AC-3 declares the wrong Control module"},{"severity":"minor","location":"apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs:666","summary":"the mklink ratchet clause is inert"},{"severity":"minor","location":"apps/rt/src/commands/review/work_removed.rs:553","summary":"the strip-is-clean assertion uses the weaker probe"}]}</VERDICT>
