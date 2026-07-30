---
id: spec.close-eleven-harness-defects-found
---

# Close eleven harness defects found by cross-checking a field report against the code, plus the session binding that makes two gates name the wrong spec: reports and gates that assert as fact something they never measured. The negative proof is made to bite, AC-per-wave coverage becomes sufficiency, the placeholder predicate stops rejecting legitimate syntax, the files section accepts tables and names the real cause, the digest stops publishing generated files as exemplars, the tautology linter reads the proof ledger before contradicting it, wave-dependency emits the declared or derived topology, the work branch record reconciles with the real branch, the session binding reaches the session the hooks read, and emit-phase confirms its transition.

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Close eleven harness defects found by cross-checking a field report against the code, plus the session binding that makes two gates name the wrong spec: reports and gates that assert as fact something they never measured. The negative proof is made to bite, AC-per-wave coverage becomes sufficiency, the placeholder predicate stops rejecting legitimate syntax, the files section accepts tables and names the real cause, the digest stops publishing generated files as exemplars, the tautology linter reads the proof ledger before contradicting it, wave-dependency emits the declared or derived topology, the work branch record reconciles with the real branch, the session binding reaches the session the hooks read, and emit-phase confirms its transition..

Why now: a field report from a real run of this pipeline listed thirteen defects. Cross-checking
each against the code changed the list. Three stated causes were refuted and would have sent the
work to the wrong file. One — the acceptance-criteria executor running under a shell where the
single quote is not a quote character — was severe enough that no textual gate in this harness
meant what it claimed to mean; it shipped first, along with the correction of a regression that
first fix introduced. Two items turned out not to be reachable from this repository at all. What
remains is eleven, and one of them the report never saw.

They share one shape, and the report named it: a component asserts as fact something it never
measured. The proof ledger calls a command red when the command could not run. The linter says a
criterion passes with or without the feature while the measurement sitting in the same directory
says otherwise. The scope reader calls a full section empty. The digest reports no miss with no
relevant candidate in the result. Two gates name a spec the author is not editing, because the
binding they read was written under a session id its writer invented.

Each fix has the same form: either measure, or say that you did not.

## Users/Stakeholders

Anyone driving this pipeline. A gate that produces confidence without producing guarantee is paid
for twice — once when a defective plan is approved, and again when the failure resurfaces later
wearing the implementer's name.

## Success Metric

Every gate touched here either reports a verdict it measured, or names what it could not measure.
No gate in the changed set can be satisfied by a command that never ran, and no gate names a spec
the author is not editing.

## Non-Goals

- **The mojibake in the active-spec listing.** No console code-page call exists anywhere in this
  repository — the enumeration returns zero — but that API affects a console handle only, and when
  stdout is a pipe it does not apply. Which case occurs has not been reproduced, and writing the
  call without reproducing it would be this spec's own defect.
- **The per-subproject context cost.** A nested guide file loads in full on first read in its
  directory, with no partial-load setting, and this harness registers no read-time injection at
  all. The only lever is how many role-pattern molds the scan scaffolds, which is a policy question
  rather than a code change.
- **Making the wave counter advance during inline execution.** The story that inline execution pins
  it was refuted — the wave-done command emits the completion event and the flow calls it. Whether
  an inline run should mark waves complete is a separate decision about a path the flow already
  documents as forbidden.

## Acceptance Criteria

Every criterion names its test and demands a non-zero pass count. A filter matching nothing exits 0
reporting `0 passed`, which reads as green — so the COUNT, not the exit code, is what each asserts.

Wave 1 — the negative proof bites:

- **AC-1** — when a close runs and a criterion is still red after its work landed, then the close reports it verbatim - taken, named, with what its column says - and is not withheld on it: QA gates the same commands moments earlier in this composite, so only the removal pass blocks
  Command: `cargo test -p mustard-rt close_reports_a_still_red_criterion_without_withholding` Expect: `[1-9][0-9]* passed`
- **AC-2** — when a close completes, then the removal pass has been taken, so a criterion that
  survives the removal of its own work is recorded rather than left a value no path produces
  Command: `cargo test -p mustard-rt close_takes_the_removal_pass` Expect: `[1-9][0-9]* passed`
- **AC-3** — when a criterion carries a Control command that is not green against the tree as it is,
  then the negative check refuses it; and when the key is absent it warns and names the id
  Command: `cargo test -p mustard-rt control_command_must_be_green_today` Expect: `[1-9][0-9]* passed`
- **AC-4** — when a wave claims a criterion whose command inspects a path that wave does not
  declare, then materialisation blocks and names the orphan paths, with wildcards expanded against
  the tree rather than compared literally
  Command: `cargo test -p mustard-rt wave_claiming_a_criterion_must_contain_its_paths` Expect: `[1-9][0-9]* passed`
- **AC-5** — when a command contains an angle bracket that is not the skeleton token the drafter
  emits, then it is executed rather than refused unrun — from a single predicate, not four copies
  Command: `cargo test -p mustard-rt placeholder_matches_the_skeleton_token_not_any_angle_bracket` Expect: `[1-9][0-9]* passed`
- **AC-6** — when the proof ledger already records a criterion red, then the tautology linter stays
  silent about it instead of asserting it passes with or without the feature
  Command: `cargo test -p mustard-rt weak_ac_defers_to_the_recorded_proof` Expect: `[1-9][0-9]* passed`

Wave 2 — every reader names what it measured:

- **AC-7** — when the files section is written as a markdown table, then its paths are read; and when
  the section has content but no path is recognised, the message says exactly that instead of
  calling the section empty, in the spec's own language
  Command: `cargo test -p mustard-rt files_section_reads_a_table_and_names_an_unreadable_one` Expect: `[1-9][0-9]* passed`
- **AC-8** — when a slice's exemplar files include a module the census classified as machine-written,
  then it is excluded; and a result whose reason is generated-only withholds its planning fields
  Command: `cargo test --workspace exemplar_files_exclude_machine_written_modules` Expect: `[1-9][0-9]* passed`
- **AC-9** — when a plan declares a wave's dependencies, then the dependency command emits those
  instead of a chain, and every edge carries the origin it came from
  Command: `cargo test -p mustard-rt wave_dependency_honours_the_declared_edges` Expect: `[1-9][0-9]* passed`
- **AC-10** — when a phase transition is recorded, then the command prints the previous and the new
  phase, and an idempotent call says so rather than staying silent
  Command: `cargo test -p mustard-rt emit_phase_confirms_the_transition` Expect: `[1-9][0-9]* passed`

Wave 3 — the record reaches whoever reads it:

- **AC-11** — when a pipeline event binds a session to a spec, then the binding lands under the
  session the hooks actually read, so a gate never names a spec the author is not editing
  Command: `cargo test -p mustard-rt session_binding_reaches_the_reading_session` Expect: `[1-9][0-9]* passed`
- **AC-12** — when the boundary gate checks an edit against a wave's file list, then the warning
  names that wave as the boundary it checked, not the parent spec
  Command: `cargo test -p mustard-rt boundary_warning_names_the_boundary_it_checked` Expect: `[1-9][0-9]* passed`
- **AC-13** — when the work branch cannot be created and the run continues on the previous branch,
  then the recorded branch is rewritten to the real one and both are named in the warning
  Command: `cargo test -p mustard-rt work_branch_record_reconciles_with_the_real_branch` Expect: `[1-9][0-9]* passed`
- **AC-14** — the project build and tests pass green
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

Wave 1 — the negative proof bites:

- `apps/rt/src/commands/pipeline/close_pipeline.rs` — the confirmation stops being advice; the removal pass gets its caller
- `apps/rt/src/commands/review/ac_negative_check.rs` — the Control pass; the placeholder predicate, hoisted to one place
- `apps/rt/src/commands/review/qa_run/mod.rs` — the single AC parser learns the Control marker
- `apps/rt/src/commands/spec/spec_scaffold.rs` — the seeded skeleton offers the new key (the seeder lives here, not in spec_draft — corrected during the wave)
- `apps/rt/src/commands/spec/approve_spec.rs` — the approval reads the Control verdict off the ledger
- `apps/rt/src/commands/wave/wave_scaffold.rs` — the orphan-path check blocks and reaches the JSON
- `apps/rt/src/commands/review/analyze_validation.rs` — the strict path recogniser; the ledger read; two of the duplicated predicates
- `apps/rt/src/commands/spec/complete_spec.rs` — the fourth copy of that predicate

Wave 1 cascades (signature/visibility ripples from the files above, declared after the boundary gate named them):

- `apps/rt/src/commands/spec/ac_add.rs` — the shared predicate's new signature
- `apps/rt/src/commands/spec/ac_amend.rs` — the shared predicate's new signature
- `apps/rt/src/commands/pipeline/plan_materialize.rs` — the sufficiency gap joins the outcome it already prints
- `apps/rt/src/commands/review/work_removed.rs` — the cached-diff filename constant, made visible to the close test

Wave 2 — every reader names what it measured:

- `apps/rt/src/commands/wave/wave_lib.rs` — the files section accepts a markdown table
- `apps/rt/src/commands/spec/scope_decompose.rs` — the diagnostic distinguishes absent from unreadable
- `packages/core/src/platform/i18n.rs` — that diagnostic stops being one hardcoded language
- `apps/scan/src/digest.rs` — exemplar files exclude machine-written modules
- `apps/rt/src/commands/feature.rs` — a generated-only result withholds its planning fields
- `apps/rt/src/commands/wave/wave_dependency.rs` — declared or derived edges, each with its origin
- `apps/rt/src/commands/event/emit_phase.rs` — the transition is confirmed on stdout

Wave 3 — the record reaches whoever reads it:

- `apps/rt/src/commands/event/emit_pipeline.rs` — the binding is written under a session the hooks read
- `apps/rt/src/shared/context.rs` — the marker's writer and its resolution
- `apps/rt/src/hooks/write/boundary_gate.rs` — the warning names the boundary it checked
- `apps/rt/src/hooks/write/work_branch_gate.rs` — the dirty-tree pre-check and the reconciliation

## Boundaries

IN: the eleven defects above, each with a test that fails before its fix and passes after. Where a
correct behaviour already exists elsewhere in this codebase — the strict path recogniser, the
dirty-tree pre-check in the other door, the deterministic success line one module over — the fix
wires that one in rather than writing a second.

A rule binding every wave, learned by shipping its opposite: a judgement ONE consumer needs goes in
that consumer, never in shared state several readers interpret differently. Before changing any
value more than one reader consumes, enumerate the readers.

OUT: the three items named in Non-Goals. Any further change to the shell the acceptance-criteria
executor spawns — that shipped separately and is what makes these criteria mean anything.

## Definitions

- **vacuous proof** — a criterion stamped `proven: red` because its command COULD NOT RUN, not because it discriminates done from not-done. The red rule is `exit != 0` alone, so an unrunnable command and a discriminating one produce the same answer.
- **sufficiency (as opposed to coverage)** — a wave that claims an acceptance criterion must CONTAIN every path that criterion's command inspects. Coverage only asks whether some wave claimed the id; sufficiency asks whether the claiming wave can actually satisfy it.
- **fabricated chain** — the `dependsOn` a wave carries when it is neither declared by the author nor derived from the import graph — a literal index expression, wave N depends on N-1, emitted unconditionally on all three code paths.
- **the exemplar leak** — the single digest projection that filters test paths but not `anchor_eligible`, so a module the census already classified as machine-written can still be published as a slice exemplar.
- **unreachable binding** — a record written under a session id the writer invented because it had none. The session-to-spec marker is written by `emit-pipeline` from the CLI, which carries no harness session id, so it lands under a placeholder directory the hooks never read — and every gate keyed on that binding silently falls back to whichever spec is newest.
- **shared-status coupling** — one consumer's need answered by changing state that several consumers read differently. The shape of the regression this spec's own preparation shipped: exit 127 was graded `skip` so the negative test would stop counting it as red proof, and `qa-run` — which tolerates a skip beside a pass — stopped blocking CLOSE on it.

## Decisions

- adopt the optional `Control:` key beside `Command:`, WARN when absent
  Reason: reversal of an earlier call in this same investigation. The earlier reasoning assumed the close-time confirmation would catch a broken command later, so a spec-format change did not pay. Once the shell defect was understood that arithmetic flipped: a Control that must be GREEN today rejects a broken regex, a shell incompatibility, a missing binary and a quoting error with one test, at PLAN time, where it costs one edit.
- correct wave-dependency's derivation rather than only labelling the edge
  Reason: labelling leaves a wrong chain standing, and that chain contradicts the levels that actually govern dispatch, which are re-derived from the wave-plan links. A consumer cannot tell an import edge from a fabricated one, and the two readings lead to opposite decisions about accepting or overriding the plan.
- a consumer-specific judgement goes in the consumer, never in the shared record's status
  Reason: learned by shipping the opposite. Grading exit 127 `skip` in the shared executor fixed the negative test and broke qa-run in the same commit. The correction reads the exit CODE in the one caller that needs the distinction, so two readers reach opposite and correct verdicts off one record. Every remaining item that touches a shared reader — the placeholder predicate, the files-section parser, the path recogniser — is bound by this.
- the boundary-gate item is re-based on the session-binding defect, not on wave counting
  Reason: the first causal story — that inline execution pins current_wave because only SubagentStop emits wave.complete — was REFUTED: `wave-done` emits it explicitly and the flow calls it. The observed symptom is explained instead by the binding landing under a placeholder session id, which sends both scope_guard and boundary_gate to the current_spec fallback and makes them name a spec the author is not editing. Reproduced live during this investigation.
- the mojibake item is investigation before code
  Reason: SetConsoleOutputCP only affects a console handle. When stdout is a pipe, which is how the harness captures it, the code page does not apply and the fix would belong in the consumer. Writing the call without reproducing which case occurs would be asserting a cause that was never measured.
- the per-subproject context cost is a /scan policy question, not a code change
  Reason: the official Claude Code documentation states that a nested CLAUDE.md loads IN FULL on first read in that directory, with no partial-load setting. This harness registers no read-time injection at all. The only lever it holds is authoring-side: how many role-pattern skills /scan scaffolds, which is uncapped today.
- every finding carries the line that holds the fact, not the signature above it
  Reason: two of the first pass's citations were wrong — one pointed at a test fixture, one at a claim the enumeration refuted — and nine more pointed at a function signature or a doc comment rather than the load-bearing line. A citation that drifts sends the work to the wrong file, which is the same defect class this spec exists to close.

## Evidence

- the confirmation pass the close takes is advisory — its own comment says the composite does not block on it
  Evidence: `apps/rt/src/commands/pipeline/close_pipeline.rs:150`
- the removal pass has no production caller: enumerating every reference outside its own module leaves the `--removal` CLI flag and the scratch-tree builder, so Removal::Survived is a value no pipeline can produce. (An earlier pass cited close_pipeline.rs:576 for this; that line is a TEST FIXTURE and the claim rests on the enumeration instead.)
  Evidence: `apps/rt/src/commands/review/cli.rs:266`
- Confirmation::Inexecutable is reached only through the executor's `skip`, which has four producers — self-invocation overwrite, spawn failure, an uncompilable Expect pattern, and command-not-found; a VALID regex that can never match is none of them, so it returns fail before and after the work and reads as 'still red'
  Evidence: `apps/rt/src/commands/review/ac_negative_check.rs:624`
- is_placeholder is the single-character test `command.contains('<')`, so JSX tags, generics and shell redirection are refused unrun
  Evidence: `apps/rt/src/commands/review/ac_negative_check.rs:572`
- the same predicate is spelled independently in the tautology linter
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:667`
- and a FOURTH time in the same function, in the no-Expect filter
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:698`
- and again in complete_spec, where it drops such criteria from the durable capability record
  Evidence: `apps/rt/src/commands/spec/complete_spec.rs:427`
- the real skeleton token spec-draft emits is a known localized literal, so the predicate can match it exactly instead of guessing at a bare angle bracket
  Evidence: `packages/core/src/platform/i18n.rs:381`
- the blocking AC-per-wave gate is a set difference over uppercased id strings; nothing about the criterion's command enters the predicate
  Evidence: `apps/rt/src/commands/wave/wave_scaffold.rs:672`
- the partial sufficiency WARN is dropped when the outcome is built, so it exists only as stderr text and no machine consumer of plan-materialize can see it
  Evidence: `apps/rt/src/commands/wave/wave_scaffold.rs:715`
- looks_like_file_path, the recogniser that WARN uses, ACCEPTS wildcards — they are then compared byte-literally against declared files, which is a guaranteed false warning on a correct plan
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:278`
- its stricter sibling rejects wildcards outright, so the correct recogniser already exists in the same file and is simply not the one wired in
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:209`
- parse_bullet accepts only a leading dash followed by a space or a tab, so a markdown table row contributes zero paths
  Evidence: `apps/rt/src/commands/wave/wave_lib.rs:144`
- heading localization is NOT the cause: the files heading resolves to both the English and the Portuguese spelling
  Evidence: `apps/rt/src/commands/spec/spec_sections.rs:33`
- the diagnostic asserts a false fact — it calls the section empty or a placeholder when the section was full — and is hardcoded Portuguese with no translate() call, regardless of the spec's language
  Evidence: `apps/rt/src/commands/spec/scope_decompose.rs:707`
- a SECOND reader of the same section exists and is format-agnostic: it keeps every body line and harvests backtick-wrapped paths from any of them, so one plan-materialize run can see zero paths from one reader and a populated list from the other
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:63`
- exemplar_files applies exactly three filters — reverse order, is_test_path, and dedup — and anchor_eligible appears nowhere in the block, while the very same function builds that class filter twelve lines below for hubs and touchpoints
  Evidence: `apps/scan/src/digest.rs:811`
- REFUTED by enumeration: the hypothesis that the ranking is recency-sensitive. Searching mtime|modified|SystemTime|metadata()|recency|churn across every source file of the scan crate returns zero hits; the only near-matches are a docstring and a test assertion message
  Evidence: `apps/scan/src/classify.rs:137`
- REFUTED: the hypothesis that generated files are not discounted. anchor_eligible gates anchors, situating anchors, hubs, touchpoints, term samples, the dictionary and rank_files — seven call sites, all production
  Evidence: `apps/scan/src/classify.rs:111`
- miss is a five-way emptiness conjunction over the projections and nothing else; no tier, score, matched-over-total ratio or confidence term participates
  Evidence: `apps/scan/src/digest.rs:659`
- the nuance-bearing field already exists — report.reason is a closed set of none, generated_only, weak, strong
  Evidence: `apps/scan/src/digest.rs:712`
- planningWithheld is `matches!(reason, "weak" | "none") && !bridged`, so generated_only keeps every planning field — and the sibling predicate non_strong DOES list generated_only, which makes the omission an asymmetry rather than a convention
  Evidence: `apps/rt/src/commands/feature.rs:100`
- the slice label is the convention's role affixes joined, filtered only for the literal core sentinel; role affixes come from token recurrence with no stopword list applied, so a language keyword can become a slice label — observed live during this investigation, label `for` at recurrence 7 with exemplars unrelated to the intent
  Evidence: `apps/scan/src/digest.rs:800`
- the tautology linter's per-part rule marks every search binary weak with no guard, no flag inspection and no direction check — though the emitted WARN requires EVERY part of a compound command to be weak, so the honest wording is that a search is weak as a STANDALONE criterion
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:437`
- its own docstring states that whether such a search can fail is a fact about the repository which only the negative test can establish; enumerating every reader of the proof ledger shows the linter is not among them, and a grep for the ledger's own name inside that file returns zero hits
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:417`
- dependsOn is a literal index expression repeated verbatim on all three emit paths and is the only edge rule in the file; even the import-DAG path discards the real topology
  Evidence: `apps/rt/src/commands/wave/wave_dependency.rs:490`
- a regression test pins the fabricated chain as correct behaviour, asserting that the second wave depends on the first regardless of what the input declared
  Evidence: `apps/rt/src/commands/wave/wave_dependency.rs:684`
- the damage is bounded: build_plan reads the wave rows off disk and assigns topological levels itself, never consuming wave-dependency's output, so the fabricated chain misleads the reader of the command during PLAN rather than the dispatcher
  Evidence: `apps/rt/src/commands/pipeline/dispatch_plan.rs:148`
- the boundary warning names the PARENT slug while the file it checked was resolved as the current wave's spec
  Evidence: `apps/rt/src/hooks/write/boundary_gate.rs:577`
- the gate uses a THIRD private parser based on backtick spans, so markdown tables are read but a path declared without backticks contributes nothing to the allowed set — and a MIXED spec is the dangerous shape, because the backticked entries make the set non-empty and every bare-declared file then warns
  Evidence: `apps/rt/src/hooks/write/boundary_gate.rs:196`
- REFUTED: the claim that only a SubagentStop observer emits pipeline.wave.complete. The wave-done command emits it explicitly, and the flow calls wave-done at the end of every wave — so the story that inline execution pins current_wave does not stand
  Evidence: `apps/rt/src/commands/pipeline/wave_done.rs:46`
- the session-to-spec marker is documented as the only binding that survives into the shipped plugin, and both scope_guard and boundary_gate resolve through it before falling back to current_spec
  Evidence: `apps/rt/src/hooks/write/scope_guard.rs:160`
- that binding is written by emit-pipeline, which run from the CLI carries no harness session id — observed live during this investigation, the marker landed in a placeholder session directory named otel-unattached while the hooks resolved through the current_spec fallback and named a spec the author was not editing, blocking an unrelated hotfix
  Evidence: `apps/rt/src/shared/context.rs:296`
- the computed work branch lives only in that session marker and in the command's stdout; enumerating the meta model shows it has no branch field at all, and the event payload does not carry it either
  Evidence: `apps/rt/src/shared/context.rs:479`
- when git refuses the checkout the gate CLEARS the marker before warning, so the intent is destroyed and nothing retries or reconciles
  Evidence: `apps/rt/src/hooks/write/work_branch_gate.rs:481`
- the other door onto the same operation already pre-checks the dirty tree and returns a loud refusal naming the paths, so the correct behaviour exists in this codebase and is simply not shared
  Evidence: `apps/rt/src/commands/work_unit_open.rs:412`
- emit-phase prints nothing on success, including on its idempotent short-circuit, so already-in-that-phase and transition-recorded are indistinguishable from stdout
  Evidence: `apps/rt/src/commands/event/emit_phase.rs:84`
- the fix pattern already exists one module over and carries a docstring explaining why silence was wrong — the one deterministic success line, added because the emitter used to succeed in total silence
  Evidence: `apps/rt/src/commands/event/emit_pipeline.rs:473`
- no console code-page call exists anywhere in the repository: enumerating SetConsoleOutputCP|CP_UTF8|chcp|SetConsoleCP across every file returns hits only in this investigation's own prose, and the read paths are strict UTF-8 or lossy while the write is a plain println
  Evidence: `apps/rt/src/commands/spec/active_specs.rs:1248`
- mold_gate is creation-only — it returns early when the target already exists — and matches by first or last filename token against the mold label, so it is inert for every edit of an existing file
  Evidence: `apps/rt/src/hooks/write/mold_gate.rs:98`
- the number of role-pattern molds scaffolded per subproject is explicitly uncapped, gated only by cluster size and exemplar count
  Evidence: `apps/rt/src/commands/scan_patterns/list.rs:16`