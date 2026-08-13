## Verdict — REJECTED (1 critical) — third review

All 12 ACs green; `cargo test --workspace` 0 failed across 34 suites; clippy 0 errors. The withdrawal is real (`carry_environment`/`link_dir`/`WorktreeConfig`/`normalise_relpaths` exist nowhere outside `#[cfg(test)]`). The previous critical (the acting collector) is genuinely closed: `Contents { ProvenEmpty, HoldsWork, Unproven }` is reachable and `Unproven` keeps, and the collector is still effective.

### CRITICAL — the refusal is blind to exactly the work this project leaves uncommitted

`apps/rt/src/commands/event/work_branch.rs:382` measures the checkout with `dirty_paths`, which drops **every** `.claude/` path (`work_unit_open.rs:238`) and returns an empty list on a failed probe (`:222`). Step 2.5 of the gate runs **only in the main checkout** (`work_branch_gate.rs:397`), where `.claude/` is NOT redirected state — it is the tracked home of the unit's `spec.md`, waves, `ac-proof.json` and review verdicts (`git check-ignore` exits 1; `git ls-files .claude/spec/…` lists them).

Live proof on this repo:
```
$ git -C /c/Atiz/mustard status --porcelain
 M .claude/spec/worktree-isolation-becomes-usable-it/change-log.md
 M .claude/spec/worktree-isolation-becomes-usable-it/change-requests.ndjson
```
Both filtered → `dirty_paths` = `[]` → `busy_checkout` = `None` → the gate runs a plain `checkout -b`. Scratch repro confirms the consequence: a first unit on `dev_first` with an uncommitted `.claude/spec/unit-one/spec.md`, `git checkout -b dev_second`, and the spec file rode along.

So AC-8's property ("the first unit's uncommitted work stays on the first unit's branch") is false for the NORMAL state of an in-flight unit between approval and `/git` — precisely the window a second session opens in.

This is the same class the wave already fixed one file over: `worktree_gc.rs` grew a whole `Contents` probe because `dirty_paths`'s carve-out and fail-open posture are wrong for a caller that destroys — and the caller that CARRIES ANOTHER UNIT'S WORK AWAY kept the blind probe. The consumer's judgement must live in the consumer.

INDEPENDENTLY CONFIRMED BY THE ORCHESTRATOR (2026-08-13) on the live checkout:
```
3 modified paths, all under .claude/spec/, all tracked (git ls-files confirms)
after the .claude/ carve-out: 0 remain  ->  busy_checkout answers None  ->  no refusal
```

### Non-blocking

- **major** — the change request approved mid-pipeline (branches as `fix/…`, `feature/…`, `hotfix/…`, base chosen by the operator) is implemented nowhere, covered by no AC, and has no follow-up spec — it exists only as a change-log line.
- **major** — commit `1a15d4bd` puts ~1.2k lines belonging to spec `2026-08-12-o-registro-por-onda` on this unit's branch, outside its Boundaries.
- **major** — the spec's own field evidence is still uncollected: `mustard-removal-mustard-31860` is kept as `"holds uncommitted work"` (guard behaving correctly; the leak needs one manual removal).
- **minor** — AC-3 declares Control `work_unit_open`; the test lives at `work_branch_gate.rs:1302`.
- **minor** — the AC-9 ratchet literal `"mklink /J"` cannot match the arg-array spelling; that clause is inert.

<VERDICT>{"verdict":"rejected","critical":1,"findings":[{"severity":"critical","location":"apps/rt/src/commands/event/work_branch.rs:382","summary":"busy_checkout measures with dirty_paths, which drops every .claude/ path and reads a failed probe as clean, so a first unit whose uncommitted work is its spec/waves/review (the normal window, and this checkout's live state) is invisible and the second unit takes the checkout anyway — AC-7/AC-8 defeated"},{"severity":"major","location":".claude/spec/worktree-isolation-becomes-usable-it/change-log.md:24","summary":"approved mid-pipeline request for fix/feature/hotfix branch naming has no AC, no code and no follow-up spec"},{"severity":"major","location":"apps/rt/src/hooks/write/boundary_gate.rs:1","summary":"commit 1a15d4bd bundles ~1.2k lines of a different spec onto this unit's branch"},{"severity":"major","location":"apps/rt/src/commands/review/work_removed.rs:325","summary":"the field leak cited as the spec's evidence is within reach but still kept as holding work"},{"severity":"minor","location":".claude/spec/worktree-isolation-becomes-usable-it/spec.md:47","summary":"AC-3 declares the wrong Control module"},{"severity":"minor","location":"apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs:659","summary":"the mklink ratchet clause is inert"}]}</VERDICT>
