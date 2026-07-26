---
id: cap.make-spec-authoring-carry-conversation
status: active
---

# make spec authoring carry conversation

### Requirement: The system SHALL satisfy the acceptance criteria of spec make-spec-authoring-carry-conversation.

#### Scenario: AC-1
- when: the clarification is finalized
- then: the marker records WHAT was settled — the grill that ran, its verdict, the terms captured, or an explicit stated reason why no grill applied — never a bare "done".
- command: `cargo test -p mustard-rt clarified_marker_records_what_was_settled`

#### Scenario: AC-2
- when: a full-scope plan is approved and its marker recorded nothing
- then: approval is REFUSED and the message says which grill to run — so the gate has a state of the world in which it fails.
- command: `cargo test -p mustard-rt approve_refuses_a_marker_that_recorded_nothing`

#### Scenario: AC-3
- when: the drafter is given the conversation material
- then: the definitions, decisions and evidence appear in the materialized spec in sections of their own, without being crammed into the prose-only opening section.
- command: `cargo test -p mustard-rt drafter_carries_conversation_material_into_its_own_sections`

#### Scenario: AC-4
- when: a finding carries a file and a line
- then: it survives materialization intact — the evidence section accepts what the prose section rejects.
- command: `cargo test -p mustard-rt evidence_section_keeps_file_and_line_references`

#### Scenario: AC-5
- when: the matched terms are generic or technical vocabulary rather than domain terms
- then: the grill declines with a stated reason instead of asking low-value questions — and the decline is a recordable outcome, not silence.
- command: `cargo test -p mustard-rt grill_declines_when_terms_are_not_domain_vocabulary`

#### Scenario: AC-6
- when: a spec grows past the old ceiling
- then: the size gate judges it by a limit derived from how a spec is loaded — read once and extracted by section — and no longer by the limit that belongs to the always-on instruction file.
- command: `cargo test -p mustard-rt spec_size_ceiling_is_not_the_instruction_file_ceiling`

#### Scenario: AC-7
- when: a wave's prompt is rendered
- then: a finding citing a file reaches the wave that declares that file and reaches NO other wave, while definitions and decisions reach every wave — so each implementer receives what concerns it and nothing more.
- command: `cargo test -p mustard-rt findings_reach_only_the_wave_that_declares_the_file`

#### Scenario: AC-8
- when: only the per-wave findings differ between two renders of the same spec
- then: the prompt's stable head is byte-identical — the material rides in the variable region, so the prompt cache is not defeated by carrying it.
- command: `cargo test -p mustard-rt carried_material_does_not_break_the_stable_prompt_head`

#### Scenario: AC-9
- when: a wave closes carrying a lesson that passes the value filter
- then: it becomes a memory file naming the wave it came from, and the NEXT round's prompt carries it — so a lesson learned in wave 1 reaches wave 3 instead of dying in the event log.
- command: `cargo test -p mustard-rt wave_lesson_reaches_the_next_round`

#### Scenario: AC-10
- when: a recorded decision does not pass the value filter
- then: it does NOT become a memory file — the filter has an input it rejects, so the memory block cannot fill with process residue.
- command: `cargo test -p mustard-rt value_filter_rejects_process_residue`

#### Scenario: AC-11
- when: the agent-prompt reference is compared with the renderer
- then: every placeholder the renderer actually substitutes appears in the reference's table — a set assertion, never a stated count, so a reworded sentence cannot break it and a silent drift cannot survive it. Folded in after the review found the guard implemented, passing, and invisible to the gate.
- command: `cargo test -p mustard-rt --test plugin_agents agent_prompt_ref_documents_every_placeholder`

#### Scenario: AC-12
- when: 
- then: the workspace builds green.
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.make-spec-authoring-carry-conversation]]

## Related

