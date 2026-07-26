---
id: spec.prove-every-acceptance-criterion-can
---

# Prove every acceptance criterion can fail before it enters the plan, and make amending one an operation of its own

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

A field run of six waves ended with two acceptance criteria out of ten already
green before a single line of the work existed. A criterion like that cannot
tell finished from untouched: it hands back confidence nobody earned. The
reliability review that followed traced it to one rule inside the criterion
linter, which treats a search-for-absence as a strong post-condition and skips
checking it. That kind of search exits zero precisely when its pattern matches
nothing, so the emptier the repository is, the greener it reads.

The linter cannot fix this on its own, and no linter could. Whether a command
is able to fail is not a property of how the command is spelled — it is a fact
about the repository the command runs against. The only honest way to establish
it is to run the criterion now, before the work exists, and require it to come
back red. A criterion that already passes at that moment has proven nothing.

The same run exposed the other half. Two criteria turned out to be written
wrong and had to be corrected mid-flight, but the artefacts are frozen once the
plan is approved, so the correction was typed in by hand and left behind only a
loose trail. There is no operation for changing a criterion, which means there
is also no place to demand that the corrected version proves itself the same
way the original was supposed to, and no place to keep what it replaced.

Both halves are the same requirement seen twice: a criterion earns its place by
demonstrating it knows how to fail, whether it is being written for the first
time or being rewritten later.

## Users/Stakeholders

The operator approving a plan, who today can be handed a criterion that will
report success no matter what happens. The implementing agents, which are
graded by those criteria. And the closing gate, which refuses to finish a spec
without a passing verification and is therefore only as trustworthy as the
criteria it runs.

## Success Metric

No criterion reaches an approved plan without a recorded red result taken
against the repository before its work existed, and correcting a criterion
leaves the superseded version and the stated reason behind it.

## Non-Goals

Re-running the proof at closing time — the proof is historical by nature, and
once the work exists the criterion is supposed to pass. Re-verifying whether
the criterion is a good description of the work; the proof answers only whether
it can fail. Touching the closing verification gate, the review verdict loop or
any other gate in the queue. Adding an environment switch to soften either new
refusal.

## Acceptance Criteria

- **AC-1** — when the criterion linter reads a search command, then no exemption lets it read strong: neither a search-for-absence nor a search chained to a step that asserts nothing escapes the weak verdict, while a genuinely combined command still does.
  Command: `cargo test -p mustard-rt --lib analyze_validation::tests::a_search_that_cannot_fail_is_never_exempt`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when a criterion's command already exits green against the tree as it is, then the negative test classifies it as vacuous instead of proven.
  Command: `cargo test -p mustard-rt --lib ac_negative_check::tests::a_command_that_passes_now_is_vacuous`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — when at least one criterion ends unproven, then the producer exits non-zero and the ledger still records the proofs it did obtain.
  Command: `cargo test -p mustard-rt --lib ac_negative_check::tests::unproven_criterion_blocks_and_records_the_proofs`
  Expect: `[1-9][0-9]* passed`
- **AC-4** — when approval is requested while a criterion carries a command with no recorded proof, then approve-spec refuses and names that criterion inside the same aggregated refusal.
  Command: `cargo test -p mustard-rt --lib approve_spec::tests::approval_refuses_a_criterion_with_no_recorded_proof`
  Expect: `[1-9][0-9]* passed`
- **AC-5** — when an amendment proposes a command that already passes, then the amendment is refused and nothing at all is written.
  Command: `cargo test -p mustard-rt --lib ac_amend::tests::amend_refuses_a_vacuous_new_command`
  Expect: `[1-9][0-9]* passed`
- **AC-6** — when an amendment is accepted, then the superseded version and the stated reason are recorded and the criterion is rewritten in every spec artefact carrying its id.
  Command: `cargo test -p mustard-rt --lib ac_amend::tests::amend_records_the_previous_version_and_rewrites_every_artifact`
  Expect: `[1-9][0-9]* passed`
- **AC-7** — when a Full plan is materialised while any criterion is still unproven, then the plan transition is withheld and the refusal arrives during planning rather than at the approval gesture.
  Command: `cargo test -p mustard-rt --lib plan_materialize::tests::an_unproven_criterion_withholds_the_plan_transition`
  Expect: `[1-9][0-9]* passed`
- **AC-8** — when approval reports on a spec whose approval marker was minted in an earlier session, then the report does not claim the current run performed that gesture.
  Command: `cargo test -p mustard-rt --lib approve_spec::tests::report_does_not_claim_a_gesture_that_did_not_happen`
  Expect: `[1-9][0-9]* passed`
- **AC-9** — when the approval question is answered with free text instead of one of the options the question offered, then no approval marker is minted whatever words that text happens to contain.
  Command: `cargo test -p mustard-rt --lib approval_marker_observer::tests::free_text_answer_never_mints_the_marker`
  Expect: `[1-9][0-9]* passed`
- **AC-10** — when the published command surface is checked, then both new commands appear in it and the shipped dispatch-loop prose names the amendment operation instead of instructing a hand edit.
  Command: `cargo test -p mustard-rt --test run_command_surface amendment_path_is_published_and_instructed`
  Expect: `[1-9][0-9]* passed`
- **AC-11** — when a verification run could not attempt almost any criterion and only an incidental one passed, then the overall verdict is not pass and no passing result is recorded for the spec.
  Command: `cargo test -p mustard-rt --lib qa_run::tests::a_run_that_verified_almost_nothing_is_not_a_pass`
  Expect: `[1-9][0-9]* passed`
- **AC-12** — when a project has no glossary at all, then the coverage report says exactly that and hands back no term list, instead of reporting zero coverage over a file nobody wrote.
  Command: `cargo test -p mustard-rt --lib glossary_coverage::tests::an_absent_glossary_is_not_a_coverage_failure`
  Expect: `[1-9][0-9]* passed`
- **AC-13** — when the terms still open are ones the corpus never published, then they are not offered for grilling and they do not block the decline that would have silenced the list.
  Command: `cargo test -p mustard-rt --lib glossary_coverage::tests::unpublished_fragments_are_not_grill_material`
  Expect: `[1-9][0-9]* passed`
- **AC-14** — when a spec carries a clarification marker that records nothing, then that is visible while listing or resuming it, not only at the moment approval is requested.
  Command: `cargo test -p mustard-rt --lib active_specs::tests::a_hollow_clarify_marker_is_visible_before_the_approval_gesture`
  Expect: `[1-9][0-9]* passed`
- **AC-15** — when a spec is approved but has not started, then the resume report carries an explicit next action instead of leaving the reader to infer it from two separate fields.
  Command: `cargo test -p mustard-rt --lib post_execute_gate::tests::an_approved_plan_that_never_started_names_its_next_action`
  Expect: `[1-9][0-9]* passed`
- **AC-16** — when the dependency pre-gate answers, then it names what it actually checked, so a green answer is not read as a guarantee it never made.
  Command: `cargo test -p mustard-rt --lib dependency_precheck::tests::the_pre_gate_names_the_scope_it_verified`
  Expect: `[1-9][0-9]* passed`
- **AC-17** — when a round is committed as one commit covering several waves, then each wave caches only the diff of the files it declared, not the whole round's.
  Command: `cargo test -p mustard-rt --lib wave_done::tests::a_wave_caches_only_its_own_declared_files`
  Expect: `[1-9][0-9]* passed`
- **AC-18** — when the whole workspace is built, then it compiles green.
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `apps/rt/src/commands/review/ac_negative_check.rs` (create)
- `apps/rt/src/commands/review/mod.rs`
- `apps/rt/src/commands/review/cli.rs`
- `apps/rt/src/commands/review/analyze_validation.rs`
- `apps/rt/src/commands/spec/ac_amend.rs` (create)
- `apps/rt/src/commands/spec/mod.rs`
- `apps/rt/src/commands/spec/cli.rs`
- `apps/rt/src/commands/spec/approve_spec.rs`
- `apps/rt/src/commands/pipeline/plan_materialize.rs`
- `apps/rt/src/hooks/observe/approval_marker_observer.rs`
- `apps/rt/src/commands/review/qa_run/mod.rs`
- `apps/rt/src/commands/glossary_coverage.rs`
- `apps/rt/src/commands/spec/active_specs.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/mod.rs`
- `apps/rt/src/commands/pipeline/resume_bootstrap/post_execute_gate.rs`
- `apps/rt/src/commands/review/dependency_precheck.rs`
- `apps/rt/src/commands/pipeline/wave_done.rs`
- `apps/rt/tests/run_command_surface.rs`
- `plugin/commands/feature.md`
- `plugin/refs/feature/full-plan.md`
- `plugin/refs/spec/resume-loop.md`
- `docs/2026-07-25-revisao-portoes-pipeline-ondas.md`

## Boundaries

IN: the negative-test engine and the command that produces the proof ledger; the
removal of the two exemptions in the criterion linter that let a search which
cannot fail read as strong — the search-for-absence rule and the blanket
compound-command escape; the plan
materialisation that runs the proof so a Full plan cannot reach the approval
gesture unproven; the approval precondition that reads the ledger as the
backstop; the correction of the approval report field that names a gesture the
current run never performed; the recogniser that decides whether an answer to
the approval question is an approval at all, which today accepts free text
carrying an approval word anywhere inside it; what a verification run is allowed
to declare when it could not attempt its criteria; the amendment command, its refusal of a vacuous
replacement and its record of the superseded version; the rewrite of an amended
criterion across every artefact of the spec that carries its id; the shipped
prose that names both operations; the queue state in the review document.

Also IN, because they are the same disease in other signals and each is a small,
localised correction rather than new capability: a coverage report that hands
back a term list when there is no glossary to cover, and that offers words the
corpus never published — the very words whose abstention blocks the decline that
would have silenced the list; a clarification marker recording nothing that is
only discovered when approval is requested; a resume report leaving the reader
to infer, from two separate fields, that an approved plan may simply start; and
a dependency pre-gate whose green answer does not say what it checked. Plus the
round's cached diff, which today carries the whole commit into every wave of the
round, including a sibling that came back blocked.

OUT: the self-invocation handling itself — which criteria a run in the product's
own process can and cannot attempt is left exactly as it is; only what the
resulting verdict is allowed to claim changes. Also out: the review verdict
loop; the wave scaffold and its freeze; every other item of the review queue
(R7, R6, R8, R3, R10); any environment switch that would soften any refusal;
re-running the proof after the work exists.

Two findings stay out because each needs a decision this spec has no basis to
make, not because they are small. First: seeding a glossary where none exists.
Nothing in the repository creates one, and choosing where it lives, who writes
the first entries and from what source is a design choice for the operator, not
a mechanical fix — this spec only stops the report from pretending the absence
is a coverage failure. Second: proving that the wave which CLAIMS a criterion is
actually scoped to satisfy it. Claiming is already enforced; adequacy is not
decidable from a declared file list, so any answer here would be a heuristic
presented as a gate, which is the exact failure this spec exists to end.

## Definitions

- **negative test** — running an acceptance criterion's own command against the repository AS IT IS NOW, before the work exists, and requiring it to FAIL. A criterion clears the proof only by failing; anything else (green, timeout, unfilled placeholder) counts as unproven.
- **vacuous criterion** — a criterion whose command already exits green before the work it describes exists, so it can never tell done from not-done. This is the single false-green in the reliability review's queue.
- **proof ledger** — <spec>/ac-proof.json — one record per criterion carrying the exact command that was run, the verdict and the exit code, plus the amendment history. It is what approve-spec reads instead of re-running the commands.
- **amendment** — a deliberate change to an already-authored acceptance criterion (its command, its evidence regex or its statement) after the spec artefacts are frozen. Today it is a hand edit; it becomes an operation of its own.
- **absence search exemption** — the rule in the tautology linter that treats rg --files-without-match / grep -L / rg -v as a strong post-condition and skips them. It is the located cause of R2.

## Decisions

- The proof is PRODUCED by an explicit command at PLAN time (ac-negative-check) and CONSUMED by approve-spec, which only reads the ledger.
  Reason: Producing it inside approve-spec would make the user wait minutes on cargo builds at the exact moment they click approve; producing it inside plan-materialize would cover the Full path only, leaving Light specs ungated. Splitting producer from consumer puts the wait where planning already waits and puts the gate on the one door both scopes pass through.
- The approval precondition is unconditional — it does not follow MUSTARD_APPROVAL_MODE.
  Reason: Project law: a gate blocks unconditionally, with no knob fork. The coverage gate in plan-materialize is the precedent (explicitly no env knob).
- The proof requirement mirrors the .clarified marker pattern already in approve_spec.rs: a marker minted by a deliberate command, classified at the door, refused when it records nothing for the current command.
  Reason: The aggregated refusal in that file already names every unmet precondition with its minting path; a third precondition joins it instead of inventing a second refusal shape.
- The absence-search exemption is removed outright rather than narrowed.
  Reason: Neither spelling is safe: --files-without-match exits 0 precisely when the pattern matches nothing, and -v exits 0 when any single line fails to match, which is true of almost any file. Whether such a search CAN fail is a fact about the repository, not about the command string — which is exactly the judgement the negative test makes and a static linter cannot.
- The new amendment command is named ac-amend, not amend.
  Reason: amend-finalize already exists for the unrelated session-end amendment window (agent/amend_finalize.rs); reusing the bare word would collide with a different meaning.
- The last criterion of a spec stays exempt from the negative test, exactly as it is exempt from the tautology linter.
  Reason: The trailing criterion is the build-green safety net; it is green before the work by design, so requiring it to fail would block every spec. Reusing the linter's existing positional rule keeps one exemption, not two.
- An amendment rewrites the criterion in EVERY artefact under the spec directory that carries that id, not only in spec.md.
  Reason: wave-plan.md and each wave spec carry the criterion lines too, and the scaffold is frozen after approval; amending only the root would leave the dispatched agent reading the superseded command.

## Evidence

- The tautology linter exempts absence searches from the weak-criterion verdict, treating them as genuine post-conditions - the located cause of the false green.
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:362`
- is_absence_search matches --files-without-match, --invert-match, -L and -v; the exemption is unconditional once any of them appears.
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:389`
- The tautology linter already exempts the LAST criterion as the trailing build-green safety net, so a positional exemption rule exists and can be reused rather than reinvented.
  Evidence: `apps/rt/src/commands/review/analyze_validation.rs:599`
- run_ac_command already executes one criterion under a per-criterion deadline and grades it by exit code plus the optional Expect regex, classifying pass / fail / timeout / skip - the negative test needs no second executor.
  Evidence: `apps/rt/src/commands/review/qa_run/runner.rs:211`
- An Expect regex that misses downgrades a green command to fail, which is what turned a zero-matching test filter red in the run that motivated this work.
  Evidence: `apps/rt/src/commands/review/qa_run/runner.rs:183`
- parse_ac_items is the single criterion parser, shared by qa-run and the linter so the two cannot drift; the negative test reads through it too.
  Evidence: `apps/rt/src/commands/review/qa_run/mod.rs:108`
- approve-spec is the one door BOTH scopes pass through, and it is deliberately fail-closed: an unreadable marker refuses instead of degrading to allow.
  Evidence: `apps/rt/src/commands/spec/approve_spec.rs:235`
- unmet_gate_message aggregates every unmet precondition into ONE refusal naming each remedy, so a third precondition joins the same message instead of adding a second refusal path.
  Evidence: `apps/rt/src/commands/spec/approve_spec.rs:263`
- plan-materialize already enforces an unconditional blocking gate (uncovered acceptance criteria) that withholds the PLAN transition and exits 2 - the precedent for a gate with no env knob.
  Evidence: `apps/rt/src/commands/pipeline/plan_materialize.rs:84`
- The wave scaffold is frozen after approval: a would-be change is skipped and only raises a drift flag, which is why an amendment cannot go through the scaffold.
  Evidence: `apps/rt/src/commands/wave/wave_scaffold.rs:578`
- wave-plan.md carries the union of the waves' acceptance lines under the same heading QA reads, so a criterion amended only in spec.md would leave a superseded copy the agents still read.
  Evidence: `apps/rt/src/commands/wave/wave_scaffold.rs:398`
- change_request.rs is the shape a deliberate record command takes here: an options struct, a report struct, a core routine testable against a temp root, and a re-read that reports the write instead of assuming it.
  Evidence: `apps/rt/src/commands/spec/change_request.rs:110`
- amend-finalize is already registered for the unrelated session-end amendment window, so the new command cannot be called amend.
  Evidence: `apps/rt/src/commands/agent/amend_finalize.rs:1`
- The published run surface is locked by a list that must be updated in the same change, and a reverse ratchet fails any registered command no prose or argv caller names.
  Evidence: `apps/rt/tests/run_command_surface.rs:28`
- The dispatch loop today tells the orchestrator to fold a behaviour change into the criteria and re-run QA, without naming any operation to do it with - the hand edit the amendment command replaces.
  Evidence: `plugin/refs/spec/resume-loop.md:82`