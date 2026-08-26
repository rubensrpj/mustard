## Verdict: REJECTED (1 critical, 1 major, 3 minor)

The reviewed repository is exactly as found (clean tree, HEAD 83b8ece1); every experiment ran in throwaway mktemp -d directories.

### Per-AC verdict
AC-1 1 passed, confirmed live end-to-end. AC-2 1 passed, confirmed live (deleted release/2026-Q3 on origin -> base went to ""). AC-3 1 passed, confirmed live: git-delete --unit release/2026-Q3 -> not-a-work-unit, branch survived; pr-list from it -> ok:true. AC-4 1 passed (executes the real binary, not a source grep). AC-5 1 passed. AC-6 3006 passed, 0 failed, exit 0; cargo clippy --workspace --all-targets -> 0 errors.

Guards: hook exits 0 and expresses blocking via permissionDecision; no new run subcommand; run output stays sorted. Molds: only review/pr_door.rs falls inside rt-report-pattern's paths and PrListReport still complies. Both spec-memory decisions implemented as recorded.

### CRITICAL — the wave hard-blocks the first edit in every project the current installer produces
In a repo with no git.flow and no --base passed, `resolve_kind_base` (work_branch.rs:59) returns `config.git.primary_base()` — the hardcoded `main` from the {main, master} fallback — WITHOUT validating it against the catalogue, so the pending marker records a branch the repo does not have. The wave's new existence probe then correctly drops it, BaseFlow::build still fills `bases` from preselected_bases() (work_kind.rs:498), base_of answers Ambiguous(["main","master"]), and the gate denies.

A/B on one identical fixture (bare origin, branches dev+producao, mustard.json = {"git":{"provider":"github"}}, emit-pipeline --kind pipeline.kind --spec sem-base --type fix, then a PreToolUse Write):

  [079e6727 baseline] no decision emitted, HEAD=fix/sem-base   (branch cut from origin/dev)
  [HEAD 83b8ece1]     "permissionDecision":"deny", HEAD=dev    (no branch, edit blocked)
    reason: "...este projeto declara várias candidatas (main, master) e nada registrou a escolha..."

Reproduced identically on a single-branch repo — the shape where git-flow.md says the router SKIPS the base row, so --base is never supplied. The deny text also names two branches the project does not have. This contradicts AC-1's own words ("inclusive num que declare exatamente uma base, ou nenhuma") and the spec's close ("em qualquer projeto"); the suite is green because no AC exercises the omitted---base path.

### Non-blocking
- major — git_settle.rs:570 (and :942): still reads flow.bases() (= preselected_bases()), so `git-settle --unit ...` refuses with "candidates":["main","master"] and "bases":["main","master"] in a repo carrying neither. Measured live.
- minor — `base-candidates` reports "primary":"main" in a repo with no main (same primary_base() root cause).
- minor — git-settle's clap help still reads "bare invocation on dev/main REFUSES" while the code now tests has_unit_record (git_cli.rs).
- minor, pre-existing (commit ee3c48ac, not this wave) — corrupted doc link at work_kind.rs:286.
