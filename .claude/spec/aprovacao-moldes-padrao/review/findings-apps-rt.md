# REVIEW — aprovacao-moldes-padrao (apps/rt)

Verdict: approved. 0 critical. Suite: 3125 passed, 0 failed, 6 ignored (78 suites).
Every acceptance criterion re-run by the reviewer AND by the orchestrator, independently.
AC-5, AC-6 and AC-7 were additionally driven against the real binary in a tempdir, so the
tolerate-in / normalise-out and the refusal were confirmed on bytes rather than assertions.

## Non-blocking observations — declared, NOT fixed in this unit

Each is a PRE-EXISTING condition or an intentional consequence, not a defect this unit
introduced. Widening the unit to absorb them is exactly the move this project has been
burned by; they are recorded here as their own lines.

1. `apps/rt/src/hooks/observe/approval_marker_observer.rs:943` — the AC-1 test opens with
   `if std::env::var_os("MUSTARD_ACTIVE_SPEC").is_some() { return; }`, so an ambient env var
   makes it pass vacuously. It is the file's own pre-existing convention (identical shape at
   `:918`, which predates this unit). Verified NOT vacuous here: the variable is unset in the
   environment the criterion was measured in. Fixing it properly means a save/restore helper
   shared by both tests — its own unit.

2. `apps/rt/src/hooks/observe/plan_approval_observer.rs:71` — the `is_full_plan ||
   already_approved` re-check is now unreachable-false, because `active_spec` guarantees fact 1
   before returning. Harmless; the `// Fact 1` comment above it now describes work already done
   one level up. Removing it is a simplification, not a defect fix.

3. `.claude/spec/aprovacao-moldes-padrao/spec.md` `## Arquivos` lists four files the waves did
   not touch: `resume_bootstrap/mode_decision.rs`, `resume_bootstrap/mod.rs`,
   `plan_approval_observer.rs`, `tests/approval_refusal_explains.rs`. The reviewer verified this
   is CORRECT rather than dropped work: `slug_of_work_branch` was already `pub(crate)` at
   `apps/rt/src/commands/event/work_branch.rs:439` so nothing needed exposing; plan mode inherits
   the fix through the shared `active_spec`; and AC-9's test landed in `approve_spec.rs`, which is
   where its own AC command looks for it. The plan over-listed; the work did not under-deliver.

## Behavioural widening, named on purpose

When rung 1 or rung 2 of the resolution walk names a spec OUTSIDE the fact-1 window, the walk
now continues and can mint on a DIFFERENT spec. That is the unit's stated Decision, not a
regression, and it is bounded by `unique_pending_full_plan`'s uniqueness requirement — zero or
more than one candidate returns `None` (fail-closed), so a real approval is never attributed to
an ambiguous spec.

## Build warnings

`cargo build --workspace` exits 0 with 4 warnings, all pre-existing in files this unit never
touched (`commands/feature.rs:488`, `apps/cli/src/commands/git_flow.rs:30`,
`shared/work_kind.rs:539`). Confirmed by `git diff dev --stat` over those paths returning empty.
