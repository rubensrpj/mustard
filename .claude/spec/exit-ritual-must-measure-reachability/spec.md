---
id: spec.exit-ritual-must-measure-reachability
---

# exit ritual must measure reachability now not history

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

A field report from a monorepo unit (sialia/btw) exposed five defects in the exit ritual — the path that runs after a pull request (PR) merges. All five share one root cause, and that is why they are one unit of work rather than five: a decision that should ask git "is this reachable right now?" instead trusts a record of something that happened once. A PR that merged one day. A submodule that committed. A log that rendered empty because a filter removed what was there.

The damage runs in the same direction every time. A branch that merged and then kept receiving commits stays "prunable" forever, so the offered command deletes refs that exist nowhere else — including a remote ref another machine moved ahead, which the sweep cannot even see because it collapses refs to a branch name before judging them. The commit step orders the submodule pointer staged unconditionally, while the rule twenty lines below explains why that pointer is wrong until the submodule PR merges; the two instructions cannot both be obeyed. The ordering rule that would prevent it governs only how PRs are opened, and on GitHub the two PRs are siblings that anyone can merge in either order. The settle manufactures its own dirty tree — checking out the base moves the pointer while the submodule directory stays put — then refuses the fast-forward because of the dirt it just made, then reports success anyway. And the wrapper that saves tokens on `git log` does it by dropping merge commits, so a range holding only merges renders as nothing at all.

Every one of these was reproduced by measurement before this spec was written, and two of the reproductions overturned a fix that had already been agreed in conversation: git's own `--ff-only` turned out to accept gitlink-only dirt, and an unconditional `git submodule update` turned out to yank a live work branch into detached HEAD. The Evidence section below carries each finding at the file and line where it was confirmed.

## Users/Stakeholders

The single operator of this harness, on every machine that runs the exit ritual — and most acutely on monorepos with submodules, where the field incident happened.

## Success Metric

The three commands that lied in the field now tell the truth: the prune advisory names only units whose EVERY ref is contained-or-covered right now; the settle either advances the base or says `ok: false`; a diff context over a merges-only range shows the merges. No new configuration knob exists.

## Non-Goals

- Fixing the rtk filter itself (source not on this machine) — the unit ships a reproduction report instead.
- Hardening the two parsers that already skip rtk's decoration (measured safe today; recorded in the report).
- Redesigning the close ritual's order or the monorepo flow — only giving the existing order teeth.

## Acceptance Criteria

- **AC-1** — when a merged PR's branch has moved (any ref: local or remote), the classifier answers the new state `moved-after-merge` and never a pruning state; per-ref evidence replaces the name-level collapse.
  Command: `cargo test -p mustard-rt moved_after_merge 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt --lib branch_state 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
- **AC-2** — when any ref of the unit moved after the merge, `git-settle` refuses (`not-merged`) and touches nothing; its gate delegates to the shared per-ref predicate (the hand-written copy in `is_merged` is gone).
  Command: `cargo test -p mustard-rt settle_refuses_when_a_ref_moved_after_merge 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt contract_refuses_on_base 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
- **AC-3** — when the only dirt is a moved gitlink, the base advance proceeds (measured: git's own `--ff-only` accepts it); after the fast-forward, `git submodule update` aligns ONLY detached submodules — a submodule sitting on any branch is reported and left untouched.
  Command: `cargo test -p mustard-rt gitlink_only_dirt 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt in_place_unit_settles 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
- **AC-4** — when the unit's own base did not advance in a finishing shape (`settled`/`partial`), the report answers `ok: false` with reason `base-behind`; `exit-and-rerun` keeps `ok: true`.
  Command: `cargo test -p mustard-rt base_behind_downgrades_ok 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `cargo test -p mustard-rt single_repo_unit_reports_itself_complete 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
- **AC-5** — the diff context reads commit ranges via `rev-list` (which rtk passes through byte-identical), never via `git log` (which rtk filters). Reproduction: non-zero today because the `log --oneline` argv is present; zero after the swap.
  Command: `cargo test -p mustard-rt diff_context_reads_ranges_via_rev_list 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `grep -n '"log", "--oneline"' apps/rt/src/commands/pipeline/diff_context.rs`
- **AC-6** — when the `/git` prose is read by a test, the gitlink stage is conditioned on reachability against the submodule's base, the pending state is named, the bump step exists, and the parent PR opens as draft with a "Blocked by" line while a submodule PR is open — asserted structurally (both halves: the new instruction present AND the unconditional MANDATORY stage gone), never by a bare word search.
  Command: `cargo test -p mustard-rt --test git_prose_rules git_prose_conditions_gitlink_on_reachability 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `grep -q "gitlink" plugin/commands/git.md`
- **AC-7** — when the same test reads the iron rules, the destructive-decision rule is there: a decision that authorises deletion reads `rev-list`, never `git log`, and states the reason (the wrapper filters `log` and passes `rev-list` through).
  Command: `cargo test -p mustard-rt --test git_prose_rules git_prose_routes_destructive_decisions_through_rev_list 2>&1 | grep -E "test result: ok\. [1-9][0-9]* passed"`
  Control: `grep -qi "iron rule" plugin/commands/git.md`
- **AC-8** — the rtk issue report exists with the two-line reproduction, and the whole workspace stays green.
  Command: `grep -q "git log --oneline -1" .claude/spec/exit-ritual-must-measure-reachability/rtk-issue-report.md && cargo test --workspace 2>&1 | tail -20 | grep -E "test result: ok"`
  Control: `cargo build --workspace`

<!-- PLAN -->

## Files

Wave 1 — rt (Rust):

- `apps/rt/src/shared/branch_state.rs` — per-ref evidence: `merged_refs` answers per REFNAME (not per name); the classifier requires every existing ref of the unit contained-or-covered; new state `moved-after-merge` (token + verdict + report); `ProviderPrCli` fetches `headRefOid` alongside `state` in the same `gh pr list` call; tests per the module's both-halves style.
- `apps/rt/src/commands/git_settle.rs` — `is_merged` delegates to the shared per-ref predicate; `update_bases` exempts gitlink-only dirt (paths from `parse_submodule_paths`), runs `git submodule update -- <path>` on detached submodules only after the fast-forward, reports each; top-level `ok: false, reason: "base-behind"` when the unit's base did not advance in a finishing shape; tests.
- `apps/rt/src/commands/pipeline/diff_context.rs` — the range read swaps `log --oneline` for `rev-list --pretty=oneline --no-commit-header` through the same `rtk_command`; source-level test pins the argv (CI has no rtk, so the pin is on the source, the style `report_module_cannot_reach_deletion` already uses).

Wave 2 — plugin (prose) + artifact:

- `plugin/commands/git.md` — commit step: gitlink stage conditioned on reachability; pr step: parent as `--draft` + "Blocked by" line while a submodule PR is open, `gh pr ready` after the bump; new iron-rule line: destructive decisions read `rev-list`, never `git log` (rtk filters log).
- `plugin/refs/git/submodule-rules.md` — the gitlink step gains the `merge-base --is-ancestor` condition and the `[pending-bump]` reading in the final status report; the close ritual gains the explicit bump + `gh pr ready` step.
- `apps/rt/tests/git_prose_rules.rs` (create) — the structural test the two prose criteria name; both-halves style, like the plugin-prose tests the repo already carries.
- `.claude/spec/exit-ritual-must-measure-reachability/rtk-issue-report.md` (create) — the reproduction report for the rtk project (log filter drops merges; the two decoration fragilities recorded).

## Boundaries

IN: the five measured defects, their tests, and the prose that instructs the same rituals.
OUT: the rtk filter's own code (source absent — the report is the deliverable); the parsers that already skip rtk's decoration (`diff_digest.rs:240`, `review_gate.rs:272`); any new configuration knob; any change to the close ritual's ORDER (only its enforcement); `pr_metrics.rs` / `status.rs` / `spec_hygiene_observer.rs` (measured: they call git directly, not through rtk).

<!-- signals: layers,files -->

## Definitions

- **contained now** — the ref is an ancestor of origin/<base> at measurement time — reachability, not history
- **covered by PR** — the ref is an ancestor of the headRefOid of a MERGED PR of that branch — the squash-merge case, where containment never holds
- **gitlink bump** — the parent commit that moves the submodule pointer to a commit already present on the submodule's base
- **per-ref verdict** — each ref of a unit (local head, each remote ref) carries its own evidence; the branch NAME only groups them

## Decisions

- Prune evidence is per REF, never per branch name: every existing ref of the unit must be contained-now or covered-by-PR
  Reason: merged_refs collapses refs to a name set — a merged local ref inserts the name even while the remote ref moved ahead, so the settle deletes a remote carrying unintegrated commits
- PrStatus::Merged alone no longer authorises pruning; a merged PR whose branch moved classifies as the new state moved-after-merge
  Reason: PR history answers what happened; pruning asks what exists — the answer ages while the branch keeps moving
- git_settle::is_merged delegates to the same per-ref predicate in branch_state (one home for the question)
  Reason: the second hand-written copy is how the two sweeps diverged before; provider evidence gains headRefOid comparison
- The gitlink enters the parent commit only when its SHA is already an ancestor of origin/<SUB_BASE>; otherwise it is a NAMED pending item and the new bump step commits the pointer after the submodule PR merges
  Reason: conditioning on 'submodule PR merged' at commit time would leave the step dead (the PR does not exist yet); reachability is the same question as finding 1, asked in the submodule
- The parent PR opens as --draft plus a 'Blocked by <sub PR url>' body line while any submodule PR is open; gh pr ready only after the bump lands
  Reason: the textual ordering rule already existed and failed — the block must be mechanical; CODEOWNERS review requests fire at ready, which is when the PR is actually complete
- Gitlink-only dirt does not block the ff-only base advance (measured: FF passes and cleans it); after the FF, git submodule update runs ONLY on detached submodules; a submodule on any branch is reported and left alone
  Reason: measured: an unconditional update yanks a submodule off its live work branch — reintroducing the very symptom finding 4 describes
- When the unit's own base did not advance in a finishing shape (action settled/partial), the report answers ok:false reason base-behind; exit-and-rerun keeps ok:true
  Reason: updated:false buried mid-JSON is what let the field incident read as success; exit-and-rerun's action field is itself the instruction, not a failure
- diff_context swaps git log --oneline for rev-list --pretty=oneline --no-commit-header through the SAME rtk_command
  Reason: the Golden Rule stays intact — measured: rtk filters log (drops merge commits, 549 vs 726 bytes on a real range) but passes rev-list through byte-identical
- The rtk filter itself is not fixable in this repo (source absent from the machine); the unit delivers rtk-issue-report.md with the minimal reproduction as an artifact
  Reason: a finding is fixed in the same pass as far as this repo CAN reach; the report is the concrete remainder, not a verbal note

## Evidence

- verdict() authorises pruning from PR history alone: `(ancestry && ahead) || pr == PrStatus::Merged` — the second arm never looks at where the branch is today
  Evidence: `apps/rt/src/shared/branch_state.rs:600`
- merged_refs collapses refs to a NAME set: a contained local ref inserts the name even when the remote ref moved ahead — the deleting side then trusts name-level evidence
  Evidence: `apps/rt/src/shared/branch_state.rs:243`
- is_merged re-implements the merge question with the same hole: gh pr list --state merged with no SHA comparison — merged once, prunable forever
  Evidence: `apps/rt/src/commands/git_settle.rs:332`
- update_bases treats ` M <sub>` as dirty-tree, but the base checkout itself manufactures that dirt (the submodule dir does not move with it); measured: merge --ff-only PASSES over gitlink-only dirt
  Evidence: `apps/rt/src/commands/git_settle.rs:353`
- the report answers ok:true while baseCheckout.updated:false sits buried mid-JSON — the base stays behind and the tree shows pre-merge file versions
  Evidence: `apps/rt/src/commands/git_settle.rs:578`
- git log --oneline through rtk drops merge commits (measured: 549 vs 726 bytes on b33d4264~3..b33d4264); a merges-only range renders EMPTY, indistinguishable from 'nothing here'; rev-list passes through byte-identical
  Evidence: `apps/rt/src/commands/pipeline/diff_context.rs:138`
- the commit step stages the moved gitlink unconditionally (MANDATORY) — the pointer recorded before the submodule PR merges names a work-branch SHA, one commit behind the real target by construction
  Evidence: `plugin/commands/git.md:40`
- twenty lines from the unconditional stage, the PR section explains why the submodule must merge FIRST — the two instructions cannot both be followed
  Evidence: `plugin/refs/git/submodule-rules.md:150`
- 'submodules before parent' governs only PR OPENING; on GitHub the two PRs are siblings and nothing blocks merging the parent first — the bump window is the delta between the two merges
  Evidence: `plugin/commands/git.md:21`
- rtk decorates two more machine-parsed outputs (a '--- Changes ---' banner on diff --name-status; a lone newline on empty diff --cached) — both parsers skip them today (no tab / empty-line filter); recorded for the rtk issue report, no code change
  Evidence: `apps/rt/src/commands/pipeline/diff_digest.rs:240`