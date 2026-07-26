---
id: wave.make-spec-authoring-carry-conversation.6-memory
---

# wave-6-memory

## Summary

Reconnect process memory: as EACH wave closes, a lesson that passes the value filter becomes a memory file naming its wave, and the next round receives it through the consumer that already exists.

## Network

- Parent: [[spec.make-spec-authoring-carry-conversation]]
- Depends on: [[wave.make-spec-authoring-carry-conversation.5-render]]

## Tasks

- [ ] The consumer is already complete and already wired: a reader that loads `{spec}/memory/*.md`, a relevance filter over the dispatch intent, a rendered block, and its place in the template — named `{cross_wave_memory}`, which says what it was built for. What is missing is the producer. Verify this first rather than trusting the claim: no spec in this repository has a `memory/` directory, so that block renders empty on every dispatch, always.
- [ ] History matters here and belongs in a code comment. This project HAD a memory injection and removed it, because the provenance it produced was confabulated. The consumer survived that removal; the producer did not, and nothing replaced it. Restoring the producer without restoring the defect is the whole job: a memory file must name the wave it came from and the run it belongs to, so every line can be traced to something that actually happened.
- [ ] Materialize at wave close, NOT at spec close. Closing the spec is too late — the following wave has already run. In the run that produced this spec, wave 1 learned that the shared git helper trims the whole output so porcelain cannot be sliced by fixed column, and waves 3, 4 and 5 all wrote Rust without ever seeing it. `wave_done` is the point that knows a wave just finished; fold the materialization there, next to the reclaim step.
- [ ] Apply the SAME value filter the emission contract already states, now on the producing side: a lesson qualifies only when there was a real choice — alternatives existed and the other way was possible — AND a future agent would decide worse without knowing it. A recap of what was done, context that was read, a file list, an interruption: none of those qualify. The filter must have an input it rejects, or it is decoration in the same way the clarification marker was.
- [ ] Scope is strictly intra-spec. This wave materializes only what happened inside THIS run, for the waves of THIS spec. No global memory is injected anywhere — project memory is read by the author while writing the spec and enters through wave 3's channel, which is a flow step, not this code path.
- [ ] Fail-open: a lesson that cannot be written must never block the wave from completing. Completion is already gated by the reclaim step for a reason that matters — stranded work — and a missing memory file is not that.
- [ ] Test `wave_lesson_reaches_the_next_round`: close a wave carrying a qualifying lesson, assert the memory file exists and names its wave, then render the next round's prompt and assert the lesson appears in it. Test `value_filter_rejects_process_residue`: a recorded decision that is a plain recap produces NO memory file — both directions, because a filter that accepts everything is the same as no filter.

## Files

- `apps/rt/src/commands/pipeline/wave_done.rs`
- `apps/rt/src/commands/agent/context_inject.rs`
