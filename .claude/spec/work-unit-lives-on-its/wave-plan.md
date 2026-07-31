---
id: wave.work-unit-lives-on-its.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.work-unit-lives-on-its.1-gate]] | gate | — | A base gate refuses to start an analysis off an integration base or on a base behind its remote, and refreshes the census there. |
| 2 | [[wave.work-unit-lives-on-its.2-spec-home]] | spec-home | [[wave.work-unit-lives-on-its.1-gate]] | The branch is cut at approval and the spec, its waves and the whole ceremony are materialized inside it; resuming from within that branch costs no ceremony. |
| 3 | [[wave.work-unit-lives-on-its.3-pr-door]] | pr-door | — | A new /mustard:pr door carries list, review and merge, absorbing what review, qa and close do today. |
| 4 | [[wave.work-unit-lives-on-its.4-unit-tools]] | unit-tools | — | git delete removes a unit whole, and a per-branch notebook collects what does not belong to the current spec. |
| 5 | [[wave.work-unit-lives-on-its.5-surface]] | surface | [[wave.work-unit-lives-on-its.3-pr-door]], [[wave.work-unit-lives-on-its.4-unit-tools]] | The exposed surface drops from fifteen doors to four: git, pr, spec and upsert. |

## Acceptance Criteria
- AC-1 — when a pipeline is opened from a checkout that is not a git.flow base, then the gate refuses and names the base to switch to. Command: `cargo test -p mustard-rt base_gate 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-2 — when a spec.md write is attempted on a protected base, then the work-branch gate refuses it instead of carving it out. Command: `cargo test -p mustard-rt spec_authoring_on_protected_base 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-3 — when /mustard:spec resolves a spec whose branch is already checked out, then it reports a no-ceremony resume rather than a confirmation prompt. Command: `cargo test -p mustard-rt resume_inside_own_branch 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-4 — when pr list runs from a work branch instead of an integration base, then it refuses and names the base. Command: `cargo test -p mustard-rt pr_list 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-5 — when a merge is requested with no recorded review verdict, then the command warns and asks rather than refusing or merging silently. Command: `cargo test -p mustard-rt pr_merge_without_verdict 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-6 — when git delete is invoked from a work branch instead of a base, then it refuses without touching anything. Command: `cargo test -p mustard-rt git_delete 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-7 — when an out-of-scope item is recorded during a work unit, then it lands in that unit's notebook and is readable back by unit. Command: `cargo test -p mustard-rt notebook 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-8 — when the exposed command surface is enumerated, then exactly four user-invocable doors remain: git, pr, spec and upsert. Command: `cargo test -p mustard-rt exposed_doors 2>&1 | grep -E "[1-9][0-9]* passed"`
- AC-9 — the project build and tests pass green. Command: `cargo build --workspace`
