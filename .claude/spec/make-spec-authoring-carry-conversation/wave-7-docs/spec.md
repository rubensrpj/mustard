---
id: wave.make-spec-authoring-carry-conversation.7-docs
---

# wave-7-docs

## Summary

Teach the flow to read project memory while authoring, to assemble the material before materializing, to record the grill outcome, and to say where each kind of material lands.

## Network

- Parent: [[spec.make-spec-authoring-carry-conversation]]
- Depends on: [[wave.make-spec-authoring-carry-conversation.4-grill]], [[wave.make-spec-authoring-carry-conversation.6-memory]]

## Tasks

- [ ] The feature flow currently reaches the drafter with a title and nothing else, and its clarify step is a bare finalize call. Rewrite that sequence: assemble the conversation material FIRST — the definitions settled, the decisions with their reasons, the findings with their evidence — then materialize, passing it through the new channel. The order matters and must be stated as an order, because a flow that materializes first invites the retype-by-hand this spec exists to remove.
- [ ] Add the PROJECT-MEMORY step to authoring, and state its scope precisely so it is not confused with the process memory wave 6 builds. While writing the spec, consult what earlier closed specs recorded, and include a memory ONLY when the author judges it relevant to THIS spec — carried in as ordinary material, with its origin named, through the same channel as everything else. It is a judgement step, not an automatic injection: an earlier automatic one was removed from this project for confabulating, and the difference that makes this safe is that a human-authored inclusion cites where it came from.
- [ ] State the grill outcome as mandatory in one of two shapes: it ran and here is its verdict, or it declined and here is the stated reason. Remove the wording that lets the finalize stand alone, because that wording is what made the marker meaningless.
- [ ] Update the glossary-grill reference so declining is documented as a first-class outcome next to the existing ones, with the honest command to record it. A reader of that page today sees only run-or-stay-silent.
- [ ] Document the per-wave delivery in the agent-prompt reference: state which material every wave receives (definitions, decisions) and which is cut by declared file (findings), and that the cut rides in the variable region so the stable head is untouched. A reader planning a wave needs to know that a finding reaches the wave that declares its file — otherwise they will not bother recording the file at all.
- [ ] Sweep for the same claim elsewhere: any flow or reference telling the reader that the clarify step needs nothing, or that the drafter takes only an intent, is now false. Fix each in this pass rather than leaving it — the previous spec found the same stale statement in four extra files, and correcting them cost minutes.
- [ ] Run the workspace build to confirm nothing else depended on the old shapes.

## Files

- `plugin/commands/feature.md`
- `plugin/refs/feature/glossary-grill.md`
- `plugin/refs/agent-prompt/agent-prompt.md`
