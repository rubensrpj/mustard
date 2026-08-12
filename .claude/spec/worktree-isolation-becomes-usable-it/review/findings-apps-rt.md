## Verdict: REJECTED — 2 critical

**ACs: all 10 green, verified myself** (`cargo build --workspace` clean; each AC filter + its control run one at a time; full workspace `4803 passed, 0 failed`; clippy: no new lints in the touched files). Negative proof in `ac-proof.json` records `proof:red` for 9/9 — honest. Molds: `rt-gate-pattern` is the only skill whose paths cover a file this wave refactored; `Ownership`/`Verb` fall outside `rt-verdict-pattern`'s declared `commands/review/**`. Guards: no new `run` subcommand (four-registration rule N/A), no `unwrap/expect` outside tests, the gate refuses by `Verdict`, observers untouched. Green tests are not the problem — these two are.

### CRITICAL 1 — `link` + any worktree removal DESTROYS the main checkout's directory (Windows)
`link_dir` (`apps/rt/src/commands/work_unit_open.rs:730`) plants a directory junction inside the worktree. Every removal path calls `git worktree remove`, which on Windows **descends the junction**:
- `apps/rt/src/commands/git_settle.rs:625` — `git worktree remove <path>` at unit close (the normal path).
- `apps/rt/src/commands/maint/worktree_gc.rs:317` — `--force`, now run with `apply=true` at **every SessionStart** (`worktree_gc.rs:492`), and `carry_environment` runs for NON-unit names too (`work_unit_open.rs:520` is unconditional), so Claude Code's own `recursing-…` trees also carry links.

Proven twice in a scratch repo, both with and without `--force`:
```
git worktree remove --force <wt>   → git removed ok      → MAIN node_modules DESTROYED
git worktree remove        <wt>    → plain remove OK      → MAIN node_modules DESTROYED
```
(control: same repo, no junction → `MAIN SURVIVED`). The prose shipped in this wave (`plugin/refs/git/git-flow.md`) actively tells operators to declare `"link": ["node_modules","target"]`, so adopting the documented example makes every unit close wipe the main checkout's dependencies. No test covers removal-after-link; AC-2 only reads through the link. Note Developer Mode is ON here (`AllowDevelopmentWithoutDevLicense=0x1`), so AC-2 exercised `symlink_dir`, not the junction most users get.

INDEPENDENTLY REPRODUCED BY THE ORCHESTRATOR (2026-08-12), isolated scratch repo, both variants:
```
--force : antes canary=True -> DEPOIS canary=False   (deps/ survives as an empty shell)
plain   : antes canary=True -> DEPOIS canary=False   (git prints NOTHING — silent)
```

### CRITICAL 2 — the second unit still takes the checkout (the gate is not the cut path)
`cut_pending_work_branch` (`apps/rt/src/commands/event/work_branch.rs:356`) does a plain `checkout_work_branch` on the MAIN checkout with **no `holds_other_work` guard**; it is what `spec-draft` calls before writing a byte (`spec_draft.rs:471` → `:737`). The codebase says so itself: `work_unit_open.rs:325-331` ("spec-draft cuts the unit's branch there at approval") and the prose this wave edited ("cut in the MAIN checkout at APPROVAL by `spec-draft`") — which now contradicts its own new table row. Either ordering is broken:
- spec-draft first → plain checkout **carries the first unit's uncommitted work** onto the second unit's branch (the exact defect AC-8 claims to close).
- gate first → `git checkout dev_second` fails (proven: `fatal: 'dev_second' is already used by worktree at …`, exit 128) → `cut_work_branch` warns and proceeds → the second unit's `spec.md`, waves and negative proof are written **on `dev_first`**.

AC-7/AC-8 pass because they call `WorkBranchGate::evaluate` directly; nothing exercises the seam that actually cuts branches.

### Non-blocking
- **major** — `open_at` (`work_unit_open.rs:379`), documented in git-flow.md as "the manual face of the same engine", never calls `carry_environment` (nor `init_submodules`): the manual face still hands back the unusable worktree this unit exists to remove.
- **minor** — `work_unit_open.rs:732` tries `symlink_dir` before the junction, inverting the recorded decision.
- **minor** — `work_branch_gate.rs:414` composes the new Deny with `format!` instead of `format_gate_message` (rt-gate-pattern) — consistent with the file's pre-existing style, so not counted.
- **minor** — commit `1a15d4bd` bundles ~1000 lines for a different spec (`boundary_gate.rs`, `wave_done.rs`, `subagent_inject.rs`).

<VERDICT>{"verdict":"rejected","critical":2,"findings":[{"severity":"critical","location":"apps/rt/src/commands/work_unit_open.rs:730","summary":"link_dir plants a junction that git worktree remove follows, so git-settle (git_settle.rs:625) and the now-acting SessionStart collector (worktree_gc.rs:317) delete the MAIN checkout's linked directory — proven empirically with and without --force"},{"severity":"critical","location":"apps/rt/src/commands/event/work_branch.rs:356","summary":"cut_pending_work_branch (the path spec-draft actually uses at approval) still plain-checkouts the second unit's branch in the main checkout with no holds_other_work guard, so the second unit takes the checkout — or, if the gate diverted first, its spec/waves/proof land on the first unit's branch"},{"severity":"major","location":"apps/rt/src/commands/work_unit_open.rs:379","summary":"open_at, documented as the manual face of the same engine, never runs carry_environment/init_submodules"},{"severity":"minor","location":"apps/rt/src/commands/work_unit_open.rs:732","summary":"symlink_dir attempted before the junction, inverting the recorded Windows decision; AC-2 never exercised the junction branch on this Dev-Mode host"},{"severity":"minor","location":"apps/rt/src/hooks/write/work_branch_gate.rs:414","summary":"new Deny text bypasses format_gate_message (rt-gate-pattern), matching the file's pre-existing style"},{"severity":"minor","location":"apps/rt/src/hooks/write/boundary_gate.rs:1","summary":"commit 1a15d4bd bundles ~1000 lines belonging to a different spec into this unit's branch"}]}</VERDICT>
