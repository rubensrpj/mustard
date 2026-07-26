---
id: wave.make-spec-authoring-carry-conversation.3-channel
---

# wave-3-channel

## Summary

Give the drafter a first-class channel for the conversation material, landing in sections of its own instead of being crammed into the prose-only opening.

## Network

- Parent: [[spec.make-spec-authoring-carry-conversation]]
- Depends on: [[wave.make-spec-authoring-carry-conversation.2-size]]

## Tasks

- [ ] The drafter's arguments are intent, scope, locale, signals, output and wave count — and the intent is documented as the title and slug seed. Nothing carries what the conversation established. Add a channel: a file argument holding structured material, so the payload is not squeezed through a shell argument and survives quoting, newlines and non-ASCII intact.
- [ ] Carry three kinds, because they behave differently. DEFINITIONS: a term and what it means here. DECISIONS: what was decided, and the reason — a decision without its reason is the thing a later reader cannot use. FINDINGS: a statement plus its evidence, and evidence means a file and a line, because that is what makes a claim checkable.
- [ ] Land them in sections of their own, registered in the section keys so every existing reader — the QA extractor, the per-wave renderer, the boundary gate — sees them through the same resolver rather than a second parser. The project has been bitten by two parsers of one section before; do not add a third.
- [ ] In `analyze_validation.rs`, the prose rule that rejects file paths in the opening section stays. What changes is the message: today it says paths belong to Root cause, Files or Tasks; now it must point at the evidence section, which is where a verified finding actually belongs. A rule that rejects without naming the destination is how the material ends up nowhere.
- [ ] Empty channel means byte-identical output to today. A spec with nothing to carry must materialize exactly as it does now — no empty headings, no placeholders. The feature must be invisible when unused.
- [ ] Test `drafter_carries_conversation_material_into_its_own_sections`: feed one definition, one decision with a reason and one finding, and assert all three appear under their own headings and that the opening section is untouched. Test `evidence_section_keeps_file_and_line_references`: a finding citing a file and a line survives materialization intact AND the validator raises no prose complaint about it — the two halves of the defect, proven together.

## Files

- `apps/rt/src/commands/spec/spec_draft.rs`
- `apps/rt/src/commands/spec/cli.rs`
- `apps/rt/src/commands/spec/spec_sections.rs`
- `apps/rt/src/commands/review/analyze_validation.rs`
