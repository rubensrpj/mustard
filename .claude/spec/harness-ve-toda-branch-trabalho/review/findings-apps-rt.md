# Review — subproject `apps/rt` — APPROVED (0 critical)

## AC gate — each named test, real counts

AC-1..AC-9 each `ok. 1 passed`; AC-10 `cargo build --workspace` exit 0. Controls
`base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` and `model_segment_strips_prefixes`
both `1 passed`. Full suite `cargo test -p mustard-rt` → **3712 passed, 0 failed (33 suites)**.
`cargo test -p mustard-core i18n_translates_work_unit_surfacing` → 1 passed. Clippy
`-p mustard-rt --all-targets`: warnings only, no `unwrap_used`/`expect_used` error.

## Effectiveness (feature enabled, not code presence)

- `git-settle --report` on this repo: 8 units classified, `awaitingPrune` = exactly the 6
  branches the field report named. Not a stub.
- `mustard-rt on SessionStart` live: `additionalContext` carries the pending-prune advisory,
  `permissionDecision: allow` — advisory, never blocking.
- Live statusline render: the prune segment, yellow, last position. Cache lands at gitignored
  `.claude/.harness/.prune-count`.

## Guards / molds

- No new `run` subcommand — `--report` is a flag on the existing `git-settle`, so the
  four-registration guard does not apply; surface test green.
- `.unwrap()`/`.expect(` in the production region of all five touched files: **0**.
- Observers untouched; `run` face reads no stdin; report JSON is BTreeMap-ordered, no timestamps.
- No new gate/check/observer/redirect module. `session_start_inject.rs` keeps its
  `Verdict::Inject` + fail-open shape — rt-inject-pattern respected.

## major findings (none blocking)

1. `git_settle.rs:625` — `repo_inventory` uses the degrading `BranchEnumerator::sweep`, but
   `branch_state.rs:148-153` documents that a consumer which REPORTS an absence must use
   `try_sweep` ("an unanswered read printed as a verified 'nothing in flight' is the same lie as
   an unmeasured PR printed as 'no PR'"). `active_specs` obeys that and sets `ok:false`; the
   `--report` face prints `"ok": true` with empty `units` either way.
2. `branch_state.rs:209` vs `git_settle.rs:326` — the reading half measures ancestry against the
   LOCAL base (`--merged dev`), the acting gate against `origin/{base}`. A local base ahead of
   its remote makes the statusline/session advisory nag for a prune that `--unit` then refuses as
   `not-merged`. Over-reports; safety still held by settle.
3. `branch_state.rs:515` — `verdict()` returns `RemoteOnly` before testing `merged_verified`, so
   a merged unit whose local ref is gone but whose REMOTE branch survives never enters
   `awaitingPrune`. Live proof: `dev_ac-executor-uses-a-real-shell` → `ancestry:true, pr:merged,
   state:"remote-only"`. The field motivation counted six REMOTAS; the aggregate only covers
   units with a local ref. (Same finding as the root reviewer's major — two independent reads.)

## minor findings

4. `branch_state.rs:539` — the branch this session is actively working on classifies as
   `draft-abandoned`; the classifier never consults HEAD.
5. `git_settle.rs:322` — `is_merged`'s docstring still names a squash-merge fallback; AC-9's test
   only scans the `//!` header, so that prose was never in range.
6. `git_settle.rs:243` — `repo_settlement` keeps a private `for-each-ref` over `refs/heads` +
   `refs/remotes/origin` (literal `origin`). "One enumerator" is true only for the two sweeps the
   spec named.
7. `git_settle.rs:553` — `alsoMergeable` now runs `is_merged` over every swept unit (local AND
   remote); `is_merged` spawns the provider CLI per non-ancestor branch, so settle's cost scales
   with branch count instead of worktree count.
