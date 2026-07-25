---
id: spec.make-spec-authoring-carry-conversation
---

# Make spec authoring carry the conversation that produced it: a first-class channel for definitions, decisions and evidence gathered before the spec is materialized, a clarification gate that can actually fail, a glossary grill that says when it does not apply, and a spec size ceiling derived from how a spec is really loaded

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

**Today.** A conversation happens: the operator states constraints, a reading of the code is verified, a hypothesis is refuted, a decision is taken and the reason for it is agreed. Then the spec is materialized — and almost none of that survives. The drafter takes one free-text argument that becomes the title, and everything else has to be typed back in by hand afterwards. What the hand does not retype is simply lost.

**Why it is a problem.** The spec is the one artifact that outlives the conversation. Every implementer reads it and never sees the discussion. When the reasoning is missing, each of them re-derives it — differently — or does not derive it at all and implements the literal words. The official guidance is explicit about what a spec owes its reader: it must name the files and interfaces involved, state what is out of scope, and end with a verification step that proves the feature works; time spent making the spec precise pays off more than time spent watching the implementation. A drafter whose only input is a title cannot produce that. A human can, by hand, if they remember — which makes quality a matter of diligence rather than of mechanism.

**Four defects, all observed in one run, all reproducible.**

1. **There is no channel.** The drafter accepts an intent, a scope, a locale, a signal list, an output path and a wave count. The intent is documented as the title and slug seed. Nothing carries definitions, decisions, refuted hypotheses, or evidence. Everything rich must be edited in after the fact.

2. **The validator actively thins what does survive.** Writing the verified facts into the opening section — with the file and line each was checked at — is rejected: the section is required to be prose, and paths and line numbers are told to live elsewhere. The rule is defensible on its own; combined with the missing channel, the result is that the most load-bearing evidence has nowhere to go.

3. **The clarification gate cannot fail.** Approving a full-scope plan requires a marker, and the command that mints it needs no term and performs no check — minting it is unconditional by design, to avoid an old deadlock. The orchestrator therefore mints the marker seconds before requesting the approval the marker unlocks. In the run that produced this spec, the marker was written with no grill of any kind having run. A gate with no state of the world in which it fails is decoration.

4. **The size ceiling was borrowed from a different artifact.** A spec warns at 200 lines and is blocked at 500 in strict mode. That 200 comes from the guidance for the always-on instruction file, whose cost is paid on every request of every session — which is exactly why it must stay small. A spec is not loaded that way: the orchestrator reads it once, and the per-wave renderer extracts named sections. The reason behind the number does not transfer, but the number did.

**Why now.** The four compound. Fixing the channel without the ceiling produces specs the gate complains about. Fixing the ceiling without the channel raises a limit nothing can reach: the richest spec this project has ever produced is 105 lines, and the largest in its history is 160 — both written by hand, well under a ceiling that was never the binding constraint. The binding constraint is that nothing carries the material in.

## Users/Stakeholders

- **The operator**, who states a constraint once in conversation and today must restate it inside the spec or watch it disappear.
- **The implementer subagents**, who receive the spec and never the discussion. They re-derive the reasoning, each in their own way, or implement the literal words without it.
- **Whoever reopens the work later** — including a future session of this same project — for whom the spec is the only surviving record of why the decisions were taken.

## Success Metric

| Metric | Target |
|---|---|
| Verified findings that reach the spec only if hand-retyped | 0 |
| Full-scope plans approved with a clarification marker that recorded nothing | 0 |
| Grills skipped in silence rather than declining explicitly | 0 |
| Spec size limits justified by an artifact loaded differently | 0 |

## Non-Goals

- **Not making specs longer for its own sake.** The goal is that what was established reaches the spec, not that prose expands. A spec with nothing to carry stays exactly as short as it is today.
- **Not removing the prose rule from the opening section.** It stays; the evidence gains a section of its own instead of being crammed where it does not belong.
- **Not removing the size gate.** It stays, with a ceiling derived from how a spec is actually loaded rather than borrowed from an artifact loaded on every request.
- **Not automating the grill itself.** Asking the operator remains the orchestrator's job in conversation. What changes is that skipping it must be recorded and justified, never silent.
- **Not touching wave decomposition, the dependency model, or dispatch.** Those are settled and out of scope here.

## Acceptance Criteria

Every criterion below names its test and demands a non-zero pass count, so a filter that matches nothing cannot report success. Each names its integration-test target where the test is not in the default one — the omission that made a criterion of the previous spec exit 0 without running anything. All nine fail today.

A note on why AC-7 exists at all: the channel alone would let the material survive the conversation but never reach the implementer, which is half the defect. The material lives once, in the parent spec, and is cut per wave at render time — definitions and decisions to every wave, a finding only to the wave that declares its file. The cut is a set intersection over the wave's declared file list, the same list the overlap audit already uses; no model decides it.

And a note on the two memories, which are deliberately different things. **Project memory** comes from earlier, closed specs, is read while the spec is being AUTHORED, and is included only when the author judges it relevant to this spec — it needs no new machinery, because it enters through the same channel as every other piece of material. **Process memory** is only what happened INSIDE this run, is materialized as each wave closes, and exists to steer the waves that follow. Keeping them apart is what makes the second one safe: a lesson from this run is checkable — it names the wave that produced it and the run it belongs to — where a general "what I have learned" summary is the confabulation that got an earlier memory injection removed from this project. AC-9 and AC-10 cover process memory only; project memory is a step in the flow, covered by the documentation wave.

- **AC-1** — when the clarification is finalized, then the marker records WHAT was settled — the grill that ran, its verdict, the terms captured, or an explicit stated reason why no grill applied — never a bare "done".
  Command: `cargo test -p mustard-rt clarified_marker_records_what_was_settled`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-2** — when a full-scope plan is approved and its marker recorded nothing, then approval is REFUSED and the message says which grill to run — so the gate has a state of the world in which it fails.
  Command: `cargo test -p mustard-rt approve_refuses_a_marker_that_recorded_nothing`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-3** — when the drafter is given the conversation material, then the definitions, decisions and evidence appear in the materialized spec in sections of their own, without being crammed into the prose-only opening section.
  Command: `cargo test -p mustard-rt drafter_carries_conversation_material_into_its_own_sections`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-4** — when a finding carries a file and a line, then it survives materialization intact — the evidence section accepts what the prose section rejects.
  Command: `cargo test -p mustard-rt evidence_section_keeps_file_and_line_references`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-5** — when the matched terms are generic or technical vocabulary rather than domain terms, then the grill declines with a stated reason instead of asking low-value questions — and the decline is a recordable outcome, not silence.
  Command: `cargo test -p mustard-rt grill_declines_when_terms_are_not_domain_vocabulary`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-6** — when a spec grows past the old ceiling, then the size gate judges it by a limit derived from how a spec is loaded — read once and extracted by section — and no longer by the limit that belongs to the always-on instruction file.
  Command: `cargo test -p mustard-rt spec_size_ceiling_is_not_the_instruction_file_ceiling`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-7** — when a wave's prompt is rendered, then a finding citing a file reaches the wave that declares that file and reaches NO other wave, while definitions and decisions reach every wave — so each implementer receives what concerns it and nothing more.
  Command: `cargo test -p mustard-rt findings_reach_only_the_wave_that_declares_the_file`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-8** — when only the per-wave findings differ between two renders of the same spec, then the prompt's stable head is byte-identical — the material rides in the variable region, so the prompt cache is not defeated by carrying it.
  Command: `cargo test -p mustard-rt carried_material_does_not_break_the_stable_prompt_head`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-9** — when a wave closes carrying a lesson that passes the value filter, then it becomes a memory file naming the wave it came from, and the NEXT round's prompt carries it — so a lesson learned in wave 1 reaches wave 3 instead of dying in the event log.
  Command: `cargo test -p mustard-rt wave_lesson_reaches_the_next_round`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-10** — when a recorded decision does not pass the value filter, then it does NOT become a memory file — the filter has an input it rejects, so the memory block cannot fill with process residue.
  Command: `cargo test -p mustard-rt value_filter_rejects_process_residue`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-11** — the workspace builds green.
  Command: `cargo build --workspace`

<!-- PLAN -->

## Files

- `apps/rt/src/commands/grill_capture.rs` — the finalize records what was settled instead of minting unconditionally
- `apps/rt/src/commands/spec/approve_spec.rs` — the gate refuses a marker with no substance
- `apps/rt/src/commands/spec/spec_draft.rs` — the channel that carries the conversation material in
- `apps/rt/src/commands/spec/cli.rs` — the drafter's new argument
- `apps/rt/src/commands/spec/spec_sections.rs` — the section keys the evidence lands in
- `apps/rt/src/commands/glossary_coverage.rs` — the grill declines explicitly when it does not apply
- `apps/rt/src/hooks/write/size_gate.rs` — the spec ceiling stops borrowing the instruction file's
- `apps/rt/src/commands/review/analyze_validation.rs` — the prose rule points at the evidence section instead of nowhere
- `apps/rt/src/commands/agent/render/sections.rs` — the per-wave cut: which material this wave receives
- `apps/rt/src/commands/agent/render/mod.rs` — the compositor wires the cut into the rendered prompt
- `apps/rt/src/commands/agent/agent_prompt_template.md` — the placeholder, in the variable region
- `apps/rt/src/commands/pipeline/wave_done.rs` — a closing wave's lesson becomes a memory file for the next round
- `apps/rt/src/commands/agent/context_inject.rs` — the value filter on the producing side, mirroring the emission contract
- `plugin/commands/feature.md` — the flow authors the material before materializing, and records the grill outcome
- `plugin/refs/feature/glossary-grill.md` — the decline is a first-class outcome

## Boundaries

IN: the files above plus their tests in the same crate.

OUT: wave decomposition, the dependency model and dispatch; the worktree isolation spec now in review; the acceptance-criteria negative test (R2 proper, queued next); automating the grill's questions — asking the operator stays the orchestrator's job in conversation.