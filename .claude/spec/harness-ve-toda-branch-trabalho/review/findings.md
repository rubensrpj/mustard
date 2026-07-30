# Review — subproject `.` — REJECTED (1 critical)

## AC roll-call
Every named test exists and returns non-zero counts (AC-1..AC-9 each `2 passed` summed over
lib+bin targets; controls `base_of_branch_reads_the_prefix_and_tolerates_worktree_prefix` and
`model_segment_strips_prefixes` both `2 passed`). `cargo build --workspace` exit 0.
Full suite `4524 passed, 6 ignored (63 suites)`. Clippy: warnings only, all pre-existing.

Guards checked: no `unwrap/expect` outside `cfg(test)`; no new `run` subcommand (only a
`--report` flag on the existing `git-settle`); `SEGMENT_KIND_COUNT` bumped to 11 with all five
palettes extended; cache file lands under gitignored `.claude/.harness/`.

## CRITICAL — a freshly cut work branch is announced as "delivered, prune me"

`apps/rt/src/shared/branch_state.rs:513` — `let merged_verified = ancestry || pr == PrStatus::Merged;`

`ancestry` comes from `for-each-ref --merged <base>`, which is TRUE for any branch with ZERO
commits ahead of its base. `work_branch_gate` cuts every unit with `checkout -b target base`
(`hooks/write/work_branch_gate.rs:197`) — no commit — so from the cut until the first
`/git commit`, the unit the user is ACTIVELY EDITING satisfies `merged_verified`.

Measured in a scratch repo (branch cut, one uncommitted edit, still standing on it):

```
$ mustard-rt run git-settle --report --root .
  "branch": "dev_fresh-unit", "state": "awaiting-prune-local",
  "ancestry": true, "pr": {"status":"unknown","reason":"provider-cli-failed"}
  "awaitingPrune": ["dev_fresh-unit"]
$ ... | mustard-rt run statusline
  ... dev_fresh-unit ?1 ... 6 a podar
```

Re-measured by the orchestrator, independently, same result: a branch cut seconds earlier with
zero commits, HEAD standing on it, classifies `awaiting-prune-local` and enters `awaitingPrune`.

The same `awaiting_prune()` feeds `session_start_inject.rs:447`, whose catalogue text tells the
agent to say the exit ritual is pending and offers `git-settle --unit <branch>` — a command
that, where `origin/<base>` sits at the cut point, passes `is_merged` and DELETES the live work
branch. This is the spec's own inversion: an undelivered unit reported as delivered, on the
default path of EVERY new unit.

Neither `prune_advisory_names_units_whose_branch_outlived_the_merge` nor
`statusline_names_units_awaiting_prune` covers the zero-commit shape — both fixtures commit first.

Suggested close: a commits-ahead check (`rev-list --count base..branch > 0`), or excluding the
checked-out branch the way `scan_work_branches` already does.

## major — `branch_state.rs:515`

`if !unit.local` returns `RemoteOnly` BEFORE the merge check, so a verified-merged unit whose
remote branch is still alive is excluded from `awaitingPrune`. Real repo:
`dev_ac-executor-uses-a-real-shell`, `ancestry:true, pr:merged, remotes:["origin"]` →
`remote-only`, absent from `awaitingPrune` — one of the "seis remotas" the Contexto exists to
surface. Re-measured by the orchestrator: confirmed verbatim.

## minor
- `git_settle.rs:594` — the reading face `report_at` lives in the same module as `-D` /
  `--delete` / `worktree remove`; AC-6's structural claim is proven only for `branch_state.rs`.
- `git_settle.rs:556` — `also_mergeable` now runs `is_merged` over every enumerated ref, and its
  provider fallback makes settle cost one network call per stale branch (was bounded by worktree
  entries).
- `tests/complete_spec_emits_qa.rs:78` — `complete_spec_emits_qa_result_event` failed in the
  first full workspace run and passed alone and on re-run; load flake, not attributable to this
  diff, but AC-10 is only reliably green on retry.
