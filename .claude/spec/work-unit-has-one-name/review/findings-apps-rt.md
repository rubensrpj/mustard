# Review — apps/rt — work-unit-has-one-name

Verdict: REJECTED · critical: 1

Verified adversarially against the BUILT BINARY, not just the unit tests.

## AC results (all commands run)

| AC | Result |
|----|--------|
| 1 | `the_base_gate_mints_the_canonical_slug` — 1 passed ×2 suites |
| 2 | `spec_draft_consumes_the_slug_it_is_given` — ok |
| 3 | `inside_work_branch_holds_when_the_gate_named_the_unit` — ok |
| 4 | `a_scaffolded_plan_is_not_reported_as_running` — ok |
| 5 | `a_declined_precheck_is_not_a_pass` — ok |
| 6 | grep gate — PASS + control |
| 7 | `the_full_path_reaches_full_plan_before_the_census_step` — ok |
| 8 | build 0 errors; `cargo test --workspace` 4704 passed 0 failed; clippy exit 0 |

Guards clean: no `unwrap`/`expect` outside `#[cfg(test)]` in new code; no new `run` subcommand; no observer returning a verdict; `main.rs` untouched; `renamedFrom`/`verdict` are conditional/additive so byte-stability holds. Mold check: `rt-verdict-pattern` sanctions `&'static str` labels over an enum when a module boundary needs the spelling AND the reason is stated — `dependency_precheck.rs:163-178` states it and each const opens with TAKEN / NEVER TAKEN. Honoured.

## CRITICAL — the one-name chain does not close on the shipped path

Reproduced end to end in a temp repo with `target/debug/mustard-rt.exe`, following `/feature`'s own order:

1. `run emit-pipeline --kind pipeline.kind --spec invented-at-dispatch --intent "Work unit has one name…"` → `{"spec":"work-unit-has-one-name","branch":"dev_work-unit-has-one-name","renamedFrom":"invented-at-dispatch"}` — the gate half works.
2. `check work_branch_gate` on the first Write (`.claude/.cache/spec-material.json`, feature.md §2) → cut the branch AND consumed `pending-work-branch`.
3. `run spec-draft --intent "Give the ok signals a verdict field" --scope light` → `"spec": "give-ok-signals-verdict-field"`. Two spec dirs on disk.
4. `run resume-bootstrap --spec give-ok-signals-verdict-field` → `insideWorkBranch : false`, standing on `dev_work-unit-has-one-name`.

**Cause:** `spec_draft.rs:481` feeds `resolve_slug` ONLY the return of `cut_work_branch`, and `cut_pending_work_branch` answers `NoPending` → `Ok(None)` once the marker is gone (`work_branch.rs:343-345`). So the "recover the slug from the branch it cut" leg is UNREACHABLE after the auto-branch hook fires — i.e. on every Full run. It never reads `current_branch`, which is the durable record its own docstring (`work_branch.rs:260-265`) names.

**The explicit-flag leg is dead too:** `grep -rn -- "--slug" plugin/ .claude/mustard/` returns nothing for `spec-draft`. `orchestrator.md:25` still passes a self-invented `--spec {slug}` plus a `"<short request>"` intent, while `feature.md:55` passes a different `"<request>"` to the draft — the exact two-argument divergence the spec's own Decisions section names as the defect.

The Success Metric ("insideWorkBranch: true from inside that branch") is NOT met.

## MAJOR (non-blocking)

1. `mode_decision.rs:150` — the REPLACEMENT docstring still asserts "So `spec` is the same string `{base}_{slug}` was built from". Disproved above. More honest than the one it replaced, but the same class of defect this unit exists to remove, one level up.
2. `plugin/commands/spec.md:25` — the mid-pipeline change request (Siglas legend gains `W{N} a iniciar`) is IMPLEMENTED but covered by no acceptance criterion: AC-4 pins only the Rust-rendered legend (`active_specs.rs:1220`), AC-6 only the approval prose. Nothing fails if the plugin legend regresses.

## Verdict by wave

Waves 2 and 3 (AC-4 … AC-7) are sound and independently verified. Wave 1 ships the mechanism but not the wiring.
