---
id: wave.exit-ritual-must-measure-reachability.2-rituals
---

# wave-2-rituals

## Summary

The /git prose stops ordering a pointer it knows is premature, and the submodule-before-parent order gains a mechanical block

## Network

- Parent: [[spec.exit-ritual-must-measure-reachability]]

## Tasks

- [ ] git.md commit step: the gitlink stage is conditioned on reachability (`git -C <SUB_ABS> merge-base --is-ancestor <gitlink sha> origin/<SUB_BASE>`) instead of being unconditional and MANDATORY; when it is not yet reachable the parent commits what is its own and the ` M <sub>` line is a NAMED pending state
- [ ] git.md + submodule-rules.md: add the explicit bump step — after the submodule PR merges, re-sample and commit the pointer alone; the final status report reads a lone ` M <sub>` as [pending-bump] when the submodule PR is still open, and as a missed step only when it already merged
- [ ] git.md pr step: while any submodule PR is open, the parent PR opens as --draft AND carries a `Blocked by <sub PR url>` line in its body; `gh pr ready` runs only after the bump lands (in the pr close ritual)
- [ ] git.md iron rules: a decision that authorises deletion reads `rev-list`, never `git log` — with the concrete reason (rtk filters `git log` and drops merge commits; a merges-only range renders EMPTY, indistinguishable from `nothing here`; rev-list passes through byte-identical)
- [ ] submodule-rules.md close section: step 1 gains the bump + gh pr ready before the parent settles
- [ ] Write apps/rt/tests/git_prose_rules.rs — the structural test the two prose criteria name, in the repo's both-halves style (the new instruction must be PRESENT and the superseded unconditional stage must be GONE, so the assertion can actually fail): git_prose_conditions_gitlink_on_reachability and git_prose_routes_destructive_decisions_through_rev_list. Register it wherever the crate's test surface is locked (tests/run_command_surface.rs guard) if that applies
- [ ] Write .claude/spec/exit-ritual-must-measure-reachability/rtk-issue-report.md — the report for the rtk project: symptom in one sentence, the two-line reproduction (`rtk git log --oneline -1 <merge sha>` answers a DIFFERENT commit than `git log --oneline -1 <merge sha>`), expected vs obtained, the impact (a destructive decision resting on an empty range), and the two decoration findings recorded as secondary (a `--- Changes ---` banner appended to diff --name-status; a lone newline on an empty diff --cached)

## Files

- `plugin/commands/git.md`
- `plugin/refs/git/submodule-rules.md`
- `apps/rt/tests/git_prose_rules.rs`
- `.claude/spec/exit-ritual-must-measure-reachability/rtk-issue-report.md`

## Reality Obligations

- **RO-2.1** — Read GitHub's official documentation for draft pull requests and confirm BOTH halves before writing the step: that a draft PR cannot be merged, and the exact `gh pr ready` invocation that clears it — the whole point of this wave is replacing a textual rule with a mechanical one, so an unverified mechanism would reproduce the defect it fixes
- **RO-2.2** — Confirm whether a draft PR defers automatic reviewer requests (CODEOWNERS); if it does, state it in the prose next to the draft step so the operator is not surprised by a review that only fires at ready
