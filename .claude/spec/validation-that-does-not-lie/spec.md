---
id: spec.validation-that-does-not-lie
---

# The Files validation answers about a project it is not looking at

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

## Context

Every plan a person approves names the files the work will touch. Before the work starts, the tool checks that list: a file the plan says it will change should already exist, and a file it says it will create should be marked as new. The check exists so a typo or a stale path is caught while it still costs nothing. A field report said the warning fired for files that were plainly there, and the operator had to open each one, confirm it existed, and dismiss the warning by hand. That is the worst outcome a check can have, because an operator who learns to dismiss warnings dismisses the true one too. Investigation found the report right about the symptom and wrong about the cause — and found that the more damaging half was invisible: on a project built with a common web framework, a large share of the listed files are never checked at all. The check says nothing about them, and silence reads exactly like approval.

## Acceptance Criteria

- **AC-1** — when the check runs from a directory other than the project root, then a file that exists is still found
  Command: `cargo test -p mustard-rt validation_resolves_from_any_working_directory -- --exact --nocapture` Expect: `1 passed`
- **AC-2** — when a plan names a path whose folder segments carry the punctuation a routing convention requires, then that path is validated rather than skipped
  Command: `cargo test -p mustard-rt validation_sees_paths_with_punctuated_segments -- --exact --nocapture` Expect: `1 passed`
- **AC-3** — when a plan mentions a term that merely looks like a path, then it is not reported as a missing file
  Command: `cargo test -p mustard-rt validation_does_not_flag_prose_as_a_file -- --exact --nocapture` Expect: `1 passed`
- **AC-4** — when a subproject still carries an uncurated rules scaffold, then the health check reports it by name
  Command: `cargo test -p mustard-rt doctor_reports_uncurated_rule_scaffolds -- --exact --nocapture` Expect: `1 passed`
- **AC-5** — when an uncurated scaffold reaches a dispatched agent as its rules, then the dispatch says so instead of passing it off as guidance
  Command: `cargo test -p mustard-rt dispatch_warns_on_uncurated_rules -- --exact --nocapture` Expect: `1 passed`
- **AC-6** — the workspace builds and its suite stays green
  Command: `cargo build --workspace && cargo test --workspace` Expect: `test result: ok`

## Root cause

Three defects in one check, plus one report that nothing consults.

1. **Two notions of "the project root" inside one process.** `apps/rt/src/commands/pipeline/plan_materialize.rs:86` resolves the root canonically through `crate::shared::context::project_dir` — the workspace anchor, then `CLAUDE_PROJECT_DIR`, then the working directory. It then calls `analyze_validation::validate`, which re-derives the root from scratch with `std::env::current_dir()`, at **two** independent sites: `apps/rt/src/commands/review/analyze_validation.rs:369` and `:416`. Off-root, two things fail at once — `ref_resolves` (`:191`) tests a bare relative path against the wrong directory, and the subproject-roots fallback comes up empty because the scan model is not found there. This tool cuts a worktree per work unit, so off-root is a normal state, not an edge case. A validator reaching into process global state also cannot be unit-tested without mutating the process, which is why the defect survived.

2. **The token scanner is blind to routing punctuation.** `analyze_validation.rs:172` accepts only `[A-Za-z0-9./-_]`, so `(`, `)`, `[`, `]`, `{`, `}` and `*` disqualify a token outright. Measured on a real spec this tool drafted for a Next.js project, four listed files were dropped without a word — two under a route group, two more under another. That project holds 96 source directories in that shape. The effect is the opposite of what the field report assumed: not a false warning on those paths, but no validation of them at all.

3. **Prose that looks like a path is reported as a missing file.** The same scanner accepts any backtick-wrapped token carrying a separator or a known extension, so a documentation-shaped mention is flagged. A spec in this repository carries one and earns a warning from the correct root. Fixing the root does not silence this class, and widening the character set for defect 2 makes it more likely — so the two must be designed together: the scanner has to become wider in what it accepts as a path and stricter in what it accepts as a *reference*.

4. **The uncurated-scaffold census exists and nothing consults it.** A freshly scanned subproject gets a rules block carrying a pending sentinel until someone writes the real rules. `apps/rt/src/commands/scan_guards/list.rs` already walks the tree and reports exactly which subprojects are still pending — but it is invoked from one place only, inside the scan enrichment loop, so a project that never re-scans never learns. Worse, `apps/rt/src/commands/agent/render/sections.rs:12` copies the rules block into a dispatched agent's prompt verbatim, so a pending scaffold arrives as that agent's guidance with nothing marking it as a placeholder.

## Files

- `apps/rt/src/commands/review/analyze_validation.rs` — take the project root as a parameter instead of re-deriving it; both `current_dir()` sites and `ref_resolves` use it. Widen the token character set to cover routing punctuation, and tighten what counts as a reference so prose stops qualifying.
- `apps/rt/src/commands/pipeline/plan_materialize.rs` — pass the root it already resolved.
- `apps/rt/src/commands/spec/spec_draft.rs` — same, at its validation call site.
- `apps/rt/src/commands/scan_guards/list.rs` — lift the pending-scaffold walk into a shared collector so the listing command and the health check share one walk.
- `apps/rt/src/commands/doctor/` — a new advisory check consulting that collector, surfaced through the existing `--check` selector; no new published command.
- `apps/rt/src/commands/agent/render/sections.rs` — warn when a pending scaffold is about to be spliced into a dispatched prompt.

## Boundaries

IN: the file-reference validation and its three causes, the pending-scaffold census wiring, the dispatch warning, and a regression test per defect. Any adjacent defect found in these files while working is fixed here, not deferred.

OUT: what the rules scaffold contains or how it is authored. The retrieval, wave and spec-graph families. Changing which warnings the validation emits beyond the three causes above.

<!-- signals: layers,files -->