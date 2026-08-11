# Review — root (.) — REJECTED (1 critical, 1 major, 2 minor)

All 10 ACs pass, each run individually plus its control (`git_settle` 52, `work_branch_gate` 21,
`prose_teaches` 10, `claude_paths` 16). Full `cargo test --workspace`: no FAILED/panicked line.
`cargo clippy --workspace --all-targets`: 0 `error:` lines.

## CRITICAL — the new prune gate permanently strands a merged unit whose base is AHEAD of origin

`git_settle.rs:635` gates the prune on `base_advanced`, which is `report["updated"] == true`.
For a base that is NOT the checked-out branch, `update_bases` computes that from
`git fetch origin <b>:<b>` (line 426), which git rejects with exit 1 when the local base is
ahead — the code even names it (`ahead-of-origin`, line 435, measured as
`merge-base --is-ancestor origin/<b> <b>`). But an ahead base DOES hold origin's tip, which is
verbatim the spec's own Definition of "base advanced" ("the local integration base holds
origin's tip after the ff-only advance"). So the authorising FACT is true and the gate reads
FALSE.

Reproduced end-to-end with the shipped binary (bases dev/main, HEAD on dev, unit `main_unit`
merged into `origin/main`, local `main` ahead by 1):

```
"ok": false,  "unit": { "merged": true, "action": "partial",
   "branchDeleted": false, "remoteDeleted": false },
"otherBases": [ { "branch": "main", "updated": false, "reason": "ahead-of-origin" } ],
"reason": "base-behind",
"nextAction": "mustard-rt run git-settle --unit main_unit"
```

Two reruns produce byte-identical output — the prescribed `nextAction` provably cannot clear it,
and the reason label is factually inverted ("behind" for a base that is ahead). No sanctioned
escape exists: `plugin/commands/git.md` forbids pushing a base directly ("PRs are the only
integration path") and forbids the manual fallback ("Never chase a refusal with a manual
`git branch -D` — the refusals are the guard"). The command's own comment at `git_settle.rs:772-778`
already records that `updated:false` conflates behind / ahead-of-origin / held-by-another-worktree;
harmless while it only labelled a report, a deadlock now that it AUTHORISES an operation.

No AC covers this case: AC-1 and AC-2 both drive `base_advanced` false via BLOCKED advances,
never via AHEAD.

The symmetric case is fine: on the current-base path `merge --ff-only` answers "Already up to
date" (exit 0) when local is ahead, so the defect is confined to the `others`/fetch path.

## MAJOR — `restoredToUnit` is new, mutating, and untested

`git_settle.rs:687-690` runs `git checkout <unit_branch>` on the failing in-place path and
reports it at line 760. `grep restoredToUnit` across `apps/rt` returns only those two hits — no
test, no AC, and it is outside the spec's declared Files-table change for this file. Verified
empirically: `restoredToUnit: true`, HEAD back on `dev_unit`, nothing pruned. It works — but a
mutation on the refusal path with zero coverage is one refactor from silently regressing.
(The apps/rt reviewer raised the same finding independently.)

## MINOR — `plugin/commands/bugfix.md:29` omits the ordering warning `/feature` spells out

It sends the flow to write `.claude/.cache/spec-material.json`, a path `is_harness_carve_out`
(`work_branch_gate.rs:141`) does NOT carve out. `/feature` §2.2 states the rule explicitly
("the base gate … comes BEFORE this write … a write from an integration base is refused and the
flow dead-ends here"); the bugfix paragraph inherits the risk without inheriting the warning.

## MINOR — `packages/core/templates/.gitignore:26` adds `spec/*/qa-report.html`, undeclared

The spec's Files table lists `scratch/`, `feature-digest.json`, `spec/*/qa-report.json`,
`spec/*/qa/`. The `.html` line is consistent and justified in the test, but was not declared.

## Orchestrator verification (independent, not the reviewer's word)

- Read `git_settle.rs:404-439` and `:605-654`: `base_advanced` is `updated == true`; the
  `others` path sets `updated:false` even when `merge-base --is-ancestor origin/<b> <b>`
  succeeds — i.e. when the base demonstrably HOLDS origin's tip. CONFIRMED by construction.
