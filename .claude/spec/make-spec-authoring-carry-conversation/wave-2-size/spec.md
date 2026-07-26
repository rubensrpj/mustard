---
id: wave.make-spec-authoring-carry-conversation.2-size
---

# wave-2-size

## Summary

Stop judging a spec by the ceiling that belongs to the always-on instruction file, and derive one from how a spec is actually loaded.

## Network

- Parent: [[spec.make-spec-authoring-carry-conversation]]

## Tasks

- [ ] The gate warns a spec at 200 lines and blocks at 500 in strict mode, sharing those thresholds with the skill gate. The 200 is the published guidance for the instruction file that loads on EVERY request of EVERY session — which is exactly why that artifact must stay small, and why an oversized one makes its own rules get ignored. A spec is not loaded that way: the orchestrator reads it once during planning, and the per-wave renderer extracts named sections. The cost is per-wave and selective, not permanent and global.
- [ ] Separate the spec thresholds from the skill thresholds — today one pair of constants serves both. A skill's ceiling is genuinely the instruction-file kind of cost (its description loads every session); leave the skill gate exactly as it is. Only the spec side moves.
- [ ] Derive the new spec ceiling from what actually degrades: the per-wave render, which extracts sections rather than the whole file. State the derivation in a comment naming the loading model, so the next reader can check the reasoning rather than the number. Do not invent a round figure and leave it unexplained — an unexplained ceiling is how the current one arrived.
- [ ] Keep the three-tier shape (advisory, stronger advisory, block-in-strict) and keep the gate blocking in strict mode. Nothing here loosens the gate's ability to refuse; it re-bases what it refuses.
- [ ] Evidence for sizing, gathered in the run that produced this spec: the richest spec this project has authored is 105 lines and the largest in its history is 160 — both hand-written, both far under the current warn tier. The ceiling was never the binding constraint; it becomes one only once wave 3 gives the drafter material to carry.
- [ ] Test `spec_size_ceiling_is_not_the_instruction_file_ceiling`: assert the spec warn tier is no longer equal to the skill warn tier, that a spec at the OLD warn line is now clean, and that the strict-mode block still fires above the new ceiling. The last assertion is the one that proves the gate did not become decoration in the act of fixing another decoration.

## Files

- `apps/rt/src/hooks/write/size_gate.rs`
