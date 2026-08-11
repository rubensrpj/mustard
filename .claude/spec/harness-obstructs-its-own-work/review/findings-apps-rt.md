# Review — apps/rt — REJECTED (1 critical, 1 major, 1 minor)

## AC results (reviewer ran every command)

AC-1..AC-10 all PASS. Controls: `git_settle` 52 passed · `work_branch_gate` 21 passed ·
`prose_teaches` 10 passed · `claude_paths` 16 passed. `cargo test --workspace` → 4782 passed,
0 failed. `cargo clippy --workspace --all-targets` → exit 0. Tests judged mutation-sensitive:
each inverts an assertion the old code satisfied (e.g. AC-3 flipped `remoteDeleted` true→false).

Guards + molds: no new `run` subcommand (four-registration rule N/A); `rt-gate-pattern` respected
— `WorkBranchGate` stays a stateless unit `Check`, the carve-out is a pure
`fn is_harness_carve_out(&str) -> bool` with self-allow before any IO; observers untouched;
`main.rs` untouched; new report fields deterministic. No violation found.

## CRITICAL — the scratch carve-out ships without its safety net in this repo

`.gitignore` (and the tracked `.claude/.gitignore`) still have no `scratch/` entry:

```
$ git check-ignore --no-index .claude/scratch/probe.sh
rc=1        # NOT ignored
```

Wave 2 made the write gate ALLOW `.claude/scratch/` (`work_branch_gate.rs:332`), but the ignore
landed only in `packages/core/templates/.gitignore`. `seed_gitignore(&claude_dir, false)` at
`packages/core/src/platform/project_seed.rs:150` PRESERVES an existing file, so no
already-initialised project — mustard included — ever receives it. Combined with the standing
iron law at `plugin/commands/git.md:10` (`add -A`, never a partial scope), the first diagnosis
that follows the new instruction at `plugin/commands/bugfix.md:19` commits its throwaway probe
into the unit. That line promises the opposite: "The seeded `.claude/.gitignore` ignores
`scratch/`, so scratch never reaches a diff and never joins the unit" — measurably false where
it ships. The spec's own Definitions say scratch is "never committed, never part of the unit".

AC-7 cannot catch this: it seeds the template into a FRESH temp repo, which is the one case that
already works. Note the implementer did add `feature-digest.json` and `spec/*/qa-report.json` to
the template — both already present in the root `.gitignore` (lines 103, 122) — so the root file
was in view; only the genuinely new entry was omitted.

## MAJOR — `restoredToUnit` is new behaviour with zero coverage

`apps/rt/src/commands/git_settle.rs:687-690` performs a real `git checkout <unit_branch>` and
`:760` publishes a new report field. `grep -n "restoredToUnit\|restored_to_unit"` over the file
returns ONLY those two lines — no test in the 52-test `git_settle` control ever asserts it is
`true`, and no AC names it. An untested git side effect on the failing path is exactly the path
this unit exists to make trustworthy.

## MINOR — template/repo disagree on QA reports

The template now ignores `spec/*/qa/`, while this repo tracks 31 files under
`.claude/spec/*/qa/` (`git ls-files … | grep -c "/qa/"` → 31). New projects will hide what this
one versions.

## What holds

The prune is gated by `base_advanced` read at `:611-616` before the prune block at `:630`; the
remote delete is inside `floor_clear` at `:671-672`; the pre-check and its gitlink exemption are
gone with `merge --ff-only` as sole authority; `merged_refs` was deleted rather than silenced.

## Orchestrator verification (independent, not the reviewer's word)

- `git check-ignore --no-index .claude/scratch/probe.sh` → rc=1 — CONFIRMED not ignored.
- `grep -n "restoredToUnit\|restored_to_unit" apps/rt/src/commands/git_settle.rs` → 2 hits
  (`:687` definition, `:760` publication) — CONFIRMED zero coverage.
- `git ls-files .claude/spec | grep -c "/qa/"` → 31 — CONFIRMED.
