---
id: wave.make-spec-authoring-carry-conversation.5-render
---

# wave-5-render

## Summary

Deliver the material to the implementer: cut it per wave at render time — definitions and decisions to every wave, a finding only to the wave that declares its file.

## Network

- Parent: [[spec.make-spec-authoring-carry-conversation]]
- Depends on: [[wave.make-spec-authoring-carry-conversation.3-channel]]

## Tasks

- [ ] Wave 3 makes the material survive the conversation; without this wave it never reaches whoever implements. The material lives ONCE, in the parent spec — a per-wave copy would drift, and this project has already been bitten by two parsers of one section. So the parent stays the single source and the cut happens at render time.
- [ ] Cut by kind, because each kind has a different natural key. Definitions are shared vocabulary and go to EVERY wave. Decisions are the law of the work — 'everything branches off dev' binds every wave — so they go to every wave too. A finding carries a file and a line, so its key is the FILE: it goes only to the wave whose declared file list contains it.
- [ ] Compute the finding cut as a set intersection over the wave's declared `## Files`, reusing the same list the overlap audit and the reference-files builder already read. Deterministic, no model in the loop, and it cannot disagree with the rest of the pipeline because it reads the same source.
- [ ] Place the material in the VARIABLE region of the prompt template, never in the prefix-stable head. The head must stay byte-identical across renders of the same spec or the prompt cache is defeated — and carrying context is worthless if it doubles the cost of every dispatch. The template's `<!-- PREFIX-STABLE -->` and `<!-- VARIABLE -->` markers are the contract; preserve them verbatim.
- [ ] Empty material renders nothing — no heading, no placeholder, no blank section. A spec that carries nothing must produce a prompt byte-identical to today's, so this wave is invisible until wave 3 has something to hand it.
- [ ] Test `findings_reach_only_the_wave_that_declares_the_file`: two waves with disjoint file lists and one finding per file; assert each wave's rendered prompt carries its own finding and NOT the sibling's, and that definitions and decisions appear in both. Both directions — a cut that lets everything through is the same as no cut. Test `carried_material_does_not_break_the_stable_prompt_head`: render the same spec twice with different findings and assert the stable head is byte-identical.

## Files

- `apps/rt/src/commands/agent/render/sections.rs`
- `apps/rt/src/commands/agent/render/mod.rs`
- `apps/rt/src/commands/agent/agent_prompt_template.md`
