---
id: spec.qa-gate-and-settle-multirepo
---

# qa gate stops silently under-verifying chatty criteria and git-settle reports every repo of the work unit

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

qa gate stops silently under-verifying chatty criteria and git-settle reports every repo of the work unit.

Anchors (from scan):
- apps/rt/src/commands/review/qa_run/runner.rs (runner, timeout)
- packages/core/src/domain/economy/sources/rtk.rs (runner)
- apps/cli/src/commands/add.rs (timeout)
- apps/rt/src/hooks/bash/native_redirect.rs (pipe)
- apps/rt/src/commands/statusline/theme.rs (pipe)
- packages/core/src/io/workspace.rs (submodule)
- apps/rt/src/commands/pipeline/wave_advance.rs (drain)
- apps/cli/src/commands/git_flow.rs (submodule)
- apps/cli/src/commands/init.rs (submodule)
- apps/rt/src/commands/pipeline/verify_pipeline.rs (timeout)
- apps/rt/src/hooks/bash/rtk_rewrite.rs (pipe)
- packages/core/src/platform/i18n.rs (pipe)

Two independent defects, both found by using the harness on itself rather than by reading it.

**The QA gate silently under-verifies.** `qa-run` spawns each acceptance criterion with its output on a pipe and only reads that pipe AFTER the process exits. Once the operating system's pipe buffer fills (about 64 KB), the command blocks writing and can never finish, so the runner burns the whole time limit on work that was already done. Proven with two criteria costing the same CPU: the one printing 1.2 MB was killed at its 20-second limit; the identical loop printing nothing passed in 110 ms. The house already fixed exactly this in `verify_pipeline.rs`, whose comment describes the symptom word for word — its sibling, which runs the same kind of command, never got the fix.

Two consequences make it worse than slow. First, the killed criterion is reported as `skip`, the same benign class as "not applicable on this platform", so the overall verdict still reads **PASS** — a gate that reports success over a criterion that never ran. Second, when the same runner is invoked in-process by `close-pipeline` or `complete-spec`, its self-invocation guard skips every criterion that would rebuild the running binary (correct — relinking a running executable fails) but still EMITS a `qa.result` verdict. Since the close gate reads the last verdict and defaults to strict, a run that verified nothing invalidates a real external pass and blocks the close.

**`git-settle` is blind to the second repository.** In a monorepo whose subproject is a git submodule, the exit ritual settles the parent and reports `"action": "settled"` with `alsoMergeable: []` — "nothing left pending" — while the submodule is still sitting on the work branch with its local and remote branches alive. The report has no field where a submodule could even appear. The contradiction is visible in one file: `plugin/commands/git.md` declares "submodules before parent, always", and its `commit`, `push` and `pr` steps each handle submodules explicitly, while `pr close`, three lines below that rule, never mentions them. Related, run from inside a submodule the command reads `<submodule>/mustard.json`, does not find it, silently falls back to the built-in default bases `{main, master}`, and refuses a `dev_` branch with `no-base-prefix` — a message that blames the branch name for a problem of location.

Note on a claim that did NOT survive checking: the field report attributed the submodule failure to the tool looking for a `.git` *folder*. It does not — it already asks git via `rev-parse`. A real parent+submodule fixture returns `no-base-prefix` (the repo opened fine) when pointed at a submodule, and `not-a-git-repo` only when the path does not exist. That is the actual trigger, and it is why the refusal messages must name what they resolved.

## Users/Stakeholders

Anyone whose acceptance criteria print more than a screenful — today those criteria are reported as passing without ever running. And anyone working in this project's own shape: a repository with a submodule, where the exit ritual currently completes half the work and says it is done.

## Success Metric

A criterion that prints megabytes is verified and reported by its real exit code; a criterion that ran out of time is never counted as a pass; and the exit ritual either settles every repository of the unit or says plainly which one it did not.

## Non-Goals

- Changing what `skip` means for a criterion that is genuinely inapplicable — only a TIMEOUT stops being lumped in with it.
- Making `close-pipeline` able to run criteria that rebuild the running binary. That is impossible, and refusing is right; it must simply stop recording a verdict it did not earn.
- Teaching `git-settle` to CHANGE anything inside a submodule automatically. This spec makes it report per repository; acting on each stays the operator's call, as the per-repo rituals already are.
- Any new flag or environment knob.

## Acceptance Criteria

- **AC-1** — when an acceptance criterion prints far more than the operating system's pipe buffer, then it completes and is judged by its real exit code instead of dying at the time limit
  Command: `cargo test -p mustard-rt ac_command_past_the_pipe_buffer_is_judged_by_exit_code`
  Expect: `1 passed`
- **AC-2** — when a criterion exceeds its time limit, then it is reported as `timeout`, never as a benign `skip`
  Command: `cargo test -p mustard-rt ac_command_killed_by_deadline_reports_timeout_not_skip`
  Expect: `1 passed`
- **AC-2B** — when any criterion timed out, then the overall verdict is never `pass`, and a real `fail` still outranks it
  Command: `cargo test -p mustard-rt overall_verdict_timeout_is_never_pass_and_never_beats_fail`
  Expect: `1 passed`
- **AC-3** — when a self-invoked run skips every criterion because it cannot rebuild the running binary, then it emits NO `qa.result` event, so an external passing verdict stays the last word
  Command: `cargo test -p mustard-rt self_invoked_all_skipped_run_writes_no_qa_result`
  Expect: `1 passed`
- **AC-3B** — when a self-invoked run finds no runnable criterion at all (no parseable acceptance criteria), then it also records no verdict — the same hole, reached by the other door
  Command: `cargo test -p mustard-rt self_invoked_empty_ac_set_writes_no_qa_result`
  Expect: `1 passed`
- **AC-4** — when `git-settle` runs from inside a submodule, then it resolves the integration bases from the superproject's `mustard.json` and recognises a `dev_` work branch
  Command: `cargo test -p mustard-rt settle_resolves_bases_from_superproject`
  Expect: `1 passed`
- **AC-5** — when a branch prefix names no known base, then the refusal names the root it resolved and the bases it knows, instead of only the branch
  Command: `cargo test -p mustard-rt no_base_prefix_names_root_and_known_bases`
  Expect: `1 passed`
- **AC-6** — when the unit spans a parent and a submodule, then the report carries one entry per repository with its own settled flag plus a global `complete`, and `complete` is false while any repository is unsettled
  Command: `cargo test -p mustard-rt settle_reports_every_repo_of_the_unit`
  Expect: `1 passed`
- **AC-7** — when the `pr close` ritual is read, then it states the submodule-first order its own iron rule promises
  Command: `cargo test -p mustard-rt pr_close_ritual_names_submodules`
  Expect: `1 passed`
- **AC-8** — the harness test suite stays green, and it runs to completion INSIDE the QA gate (the chatty-output proof, end to end)
  Command: `cargo test -p mustard-rt`
  Expect: `test result: ok`

<!-- PLAN -->

## Files

- `apps/rt/src/shared/proc.rs` (modify) — one spawn-with-drained-pipes helper, extracted from the working precedent. Corrected during EXECUTE: this file already existed as the shared process-primitives module, so the helper joins it instead of creating a sibling
- `apps/rt/src/commands/review/qa_run/render.rs` (modify) — test locking the new verdict class in `qa/report.md`
- `apps/rt/src/shared/mod.rs` (modify) — register the helper module
- `apps/rt/src/commands/review/qa_run/runner.rs` (modify) — use the helper; report a timeout as `timeout`
- `apps/rt/src/commands/review/qa_run/mod.rs` (modify) — overall verdict counts a timeout as not-pass; suppress the verdict event when a self-invoked run verified nothing
- `apps/rt/src/commands/pipeline/verify_pipeline.rs` (modify) — call the shared helper instead of its private copy
- `apps/rt/src/commands/git_settle.rs` (modify) — superproject base resolution, diagnostic refusals, per-repo report
- `apps/rt/tests/run_command_surface.rs` (modify) — the `pr close` ritual doc guard
- `plugin/commands/git.md` (modify) — `pr close` states the submodule-first order
- `plugin/refs/git/submodule-rules.md` (modify) — the close ritual per repository
- `MUSTARD-COMMANDS.md` (modify) — added during REVIEW: the third instruction surface still described `pr close` as a single-repo ritual, fourteen lines under its own "submodules before parent" iron rule

## Boundaries

IN: the two defects above, their tests, and the two instruction surfaces that describe the close ritual.
OUT: automatic mutation of a submodule by `git-settle`; the meaning of `skip` for inapplicable criteria; the `dirty-tree` refusal, which is correct and stays (only its visibility travels with the per-repo report); any new flag, mode or environment variable.