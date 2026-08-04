# Review 2 (after the critical fix) — apps/rt — work-unit-has-one-name

Verdict: approved · critical: 0

Verified adversarially: full diff read, every AC command AND every Control run, and the shipped path reproduced end to end against `target/debug/mustard-rt.exe` — not just unit tests.

## AC results — all commands run

| AC | Real output |
|----|-------------|
| 1 | `the_base_gate_mints_the_canonical_slug` — ok. 1 passed ×2 suites |
| 2 | `spec_draft_consumes_the_slug_it_is_given` — ok. 1 passed ×2 |
| 3 | `inside_work_branch_holds_when_the_gate_named_the_unit` — ok. 1 passed ×2 |
| 4 | `a_scaffolded_plan_is_not_reported_as_running` — ok. 1 passed ×2 |
| 5 | `a_declined_precheck_is_not_a_pass` — ok. 1 passed ×2 |
| 6 | grep gate AC6_PASS; control ×4 |
| 7 | `the_full_path_reaches_full_plan_before_the_census_step` — ok. 1 passed |
| 8 | build 0 errors; `cargo test --workspace` exit 0; clippy exit 0 |

All six Controls emit `[1-9][0-9]* passed`. The three build/clippy warnings are pre-existing in untouched files.

## The previous CRITICAL is genuinely closed

Reproduced in a temp repo following feature.md's own order: gate → `{"spec":"work-unit-has-one-name","branch":"dev_work-unit-has-one-name","renamedFrom":"invented-at-dispatch"}`; `check work_branch_gate` on the first Write cut the branch and consumed `pending-work-branch`; `run spec-draft --intent "Give the ok signals a verdict field"` **with no `--slug`** → `"spec": "work-unit-has-one-name"`, ONE spec dir; `run resume-bootstrap` → `insideWorkBranch : true`.

The `current_branch` leg added in d791199d is what closes it, and AC-3's test drives exactly that leg (`slug: None`, marker absent). Both prose call sites now carry `--slug`.

Live spot-checks: `run dependency-precheck` on a `.cs` spec emits `"verdict":"declined"` beside `"ok":true`/`"skipped":"stack-unsupported"`; `run active-specs` prints `W1 a iniciar` with the legend line.

## Guards + molds

Clean. No new `run` subcommand; no `unwrap`/`expect` outside `#[cfg(test)]`; no observer returning a verdict; `main.rs` untouched; `renamedFrom`/`verdict` additive and conditional with the full `insta` suite green, so byte-stability holds. `rt-verdict-pattern` honoured — the skill sanctions `&'static str` labels when a module boundary needs the spelling AND the reason is stated; `dependency_precheck.rs:158-178` states it.

## Findings (none blocking)

1. **MAJOR — `apps/rt/tests/spec_flow_prose.rs:330`.** The mid-pipeline change request (picker Siglas legend) was implemented and pinned by `the_picker_legend_names_the_not_yet_started_status`, which passes. But `spec.md` still carries only AC-1…AC-8 and `ac-proof.json` the same 8 ids, so `qa-run` will never execute it. Worse: that test's docstring cites **"AC-10"**, an id that exists nowhere in this spec — a comment guaranteeing a ledger entry that does not exist, in the very unit whose thesis is that such a comment is worse than none. AC-7's Control (`--test spec_flow_prose | grep "[1-9][0-9]* passed"`) still matches when one test in the binary fails, so no AC gate catches a legend regression either. Same gap for `the_draft_call_carries_the_name_the_gate_minted` (no id at all).

2. **MAJOR — `apps/rt/src/commands/spec/active_specs.rs:1198`.** The row pads with `{:<10}` against the hardcoded 10-wide `Status` header at line 1172. `W1 a iniciar` is 12 chars (`W10 a iniciar` 13), so `Onde`/`Resumo` shift right on every scaffolded-plan row. Observed live.

3. **MINOR — `spec.md` AC-8.** Its text says "the project build and tests pass green"; its `Command:` runs only `cargo build --workspace`. Pre-existing authoring, but it is itself an ok-signal reporting on a pass it never took. (`cargo test --workspace` run separately: exit 0.)

The four defects the unit set out to remove are fixed, proven against the built binary, and none of the findings above is a Guard breach, a mold breach, or a wrong answer.
