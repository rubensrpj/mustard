## Verdict — spec `close-eleven-harness-defects-found` (commits d7127c0a + 5ebb1ba4): APPROVED

**Guards (`apps/rt/CLAUDE.md`) — PASS.** No new `unwrap`/`expect` outside `#[cfg(test)]` (clippy deny lints produced zero errors); both touched gates keep fail-open degradation and express blocking only via `Verdict`; no new `run` subcommand, so the four-registration rule is not in play; `emit-phase`'s new success line is deterministic (no timestamp/session).

**Mold `rt-gate-pattern` — PASS.** `boundary_gate.rs` and `work_branch_gate.rs` stay unit-struct `Check`s, mode cascade untouched, in-file tests added for the new paths, `[bracketed-tag]`/existing message conventions kept.

**Acceptance criteria — 14/14 PASS, each independently re-run:**
- AC-1 `close_reports_a_still_red_criterion_without_withholding` — 1 passed (×2 binaries). Advisory direction matches the recorded decision (never blocks on `NotTaken`); only the removal pass refuses.
- AC-2 `close_takes_the_removal_pass` — 1 passed. `take_removal` now `pub(crate)`, called from the close; `Removal::Survived` refuses, engine error does not.
- AC-3 `control_command_must_be_green_today` — 1 passed. Red control refuses before the red proof; absent control WARNs by id (`control_missing`); late-added control is re-taken (`recontrol`).
- AC-4 `wave_claiming_a_criterion_must_contain_its_paths` — 1 passed. Gap 4 now blocks, reaches `plan-materialize` JSON (`criteria_outside_claimants`, exit 2); wildcards matched via the crate's one `glob_match`.
- AC-5 `placeholder_matches_the_skeleton_token_not_any_angle_bracket` — 1 passed. Single `qa_run::is_skeleton`; all four `contains('<')` sites replaced (remaining hits are a test fixture and an unrelated scan-patterns matcher).
- AC-6 `weak_ac_defers_to_the_recorded_proof` — 1 passed. Linter reads the ledger via `load_ledger`/`recorded_proof`/`evidenced`, fail-open toward keeping the WARN.
- AC-7 `files_section_reads_a_table_and_names_an_unreadable_one` — 2 passed (wave_lib table parser + scope_decompose diagnostic); message via `translate()` in the spec's locale, PT literal removed.
- AC-8 `cargo test --workspace exemplar_files_exclude_machine_written_modules` — 1 passed (scan) + rt sibling; `anchor_eligible` filter added to `exemplar_files`; `generated_only` now withholds planning fields.
- AC-9 `wave_dependency_honours_the_declared_edges` — 1 passed. Declared edges honoured (number or wave name), import path emits real topology, `dependsOn` stays a numeric array with sibling `dependsOnOrigin` (matches the recorded decision); the old chain-pinning regression test was corrected.
- AC-10 `emit_phase_confirms_the_transition` — 1 passed. Prints `{ok,kind,spec,from,to}`, idempotent call says `idempotent: true`; print lives only in the CLI entry.
- AC-11 `session_binding_reaches_the_reading_session` — 1 passed. `is_placeholder_session` is the single predicate; `otel-unattached` duplication is the documented decision; resolver skip + writer refusal both asserted.
- AC-12 `boundary_warning_names_the_boundary_it_checked` — 1 passed. Warning names `{spec}/{wave-dir}`; mixed backtick/bare Files sections harvested through the shared strict recogniser.
- AC-13 `work_branch_record_reconciles_with_the_real_branch` — 1 passed. Failed checkout rewrites the marker to the real branch, names both plus dirty paths; protected-branch deny keeps the marker for retry.
- AC-14 — `cargo build --workspace` green; full `cargo test --workspace` has zero failing test binaries; clippy: warnings only, all pre-existing debt (never touched `cargo fmt`).

**Change requests — all addressed.** The test-naming instruction is proven: every AC filter now matches ≥1 test (Wave 1's zero-match failure mode is closed).

Non-blocking notes:
- minor — `apps/rt/src/commands/wave/wave_scaffold.rs:610`: AC-4 says wildcards "expanded against the tree"; implementation pattern-matches instead (deliberate, documented, keeps byte-stable output; intent satisfied).
- minor — `apps/rt/src/commands/wave/wave_scaffold.rs:612`: command layer importing `glob_match` from `hooks::write::boundary_gate` inverts the usual dependency direction; a shared util home would be cleaner.
- minor — `apps/rt/src/hooks/write/work_branch_gate.rs:87`: new user-facing strings hardcoded in Portuguese (follows that file's pre-existing convention rather than the i18n catalogue).

<VERDICT>{"verdict":"approved","critical":0,"findings":[{"severity":"minor","location":"apps/rt/src/commands/wave/wave_scaffold.rs:610","summary":"AC-4 wildcards are glob-matched, not tree-expanded — deliberate, documented, intent satisfied"},{"severity":"minor","location":"apps/rt/src/commands/wave/wave_scaffold.rs:612","summary":"command layer imports glob_match from hooks::write::boundary_gate; shared util would be cleaner"},{"severity":"minor","location":"apps/rt/src/hooks/write/work_branch_gate.rs:87","summary":"new user-facing gate strings hardcoded PT instead of i18n catalogue (file convention)"}]}</VERDICT>
