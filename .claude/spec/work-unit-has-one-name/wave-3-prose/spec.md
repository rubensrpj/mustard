---
id: wave.work-unit-has-one-name.3-prose
---

# wave-3-prose

## Summary

The flow documents stop promising two things they cannot deliver: an approval from a bare letter, and a Full path whose first census read can only abstain.

## Network

- Parent: [[spec.work-unit-has-one-name]]
- Depends on: [[wave.work-unit-has-one-name.1-identity]], [[wave.work-unit-has-one-name.2-signals]]

## Tasks

- [ ] The picker's selection block tells the user that typing `ar` mints the approval marker. That is TRUE only in the one-step form (`/mustard:spec ar` submitted as the whole prompt) and FALSE in the two-step form the table itself opens — where the user types a bare letter, which never reaches the observer. Measured live: the bare letter minted nothing and the flow silently fell back to the full approval round.
- [ ] Fix the PROSE, not the observer. The observer requires the exact whole-prompt form on purpose: a substring rule would let a sentence merely QUOTING the form mint an approval, and that forgery already happened once on another door. The marker's whole value is being unforgeable. So the table must name the form that works — `/mustard:spec ar` typed in full — and stop claiming the bare letter does it. Use the words `typed in full` so the criterion can see it.
- [ ] Say WHY in one clause. A reader who is told to type more without being told why will shorten it again.
- [ ] `feature.md` orders `plan-prepare` right after the draft, but on the Full path the `## Files` census it reads is only authored later, in the full-plan machinery. So the first call returns scope:abstain with filesSectionEmpty:true EVERY time — measured on this very run. Mark in §2 that the full path continues in the full-plan document BEFORE the census-dependent step. Use the words `full path continues in` so the criterion can see it.
- [ ] The same two documents disagree about materialisation: one describes `spec-draft` without `--plan`, the other states the correct first materialisation is `spec-draft --plan` in ONE call. Following the first lands you in `plan-materialize`, which the second classifies as the EDIT door. Name `spec-draft --plan` in feature.md so the two agree.
- [ ] Add the prose ratchet the criterion names: the_full_path_reaches_full_plan_before_the_census_step, in the existing spec-flow prose test. It must assert BOTH halves this repo's prose tests always assert — the text names the mechanism where the reader arrives, AND the mechanism still exists in code.
- [ ] Re-read the no-ceremony paragraph in the resume loop reference. Wave 1 makes its promise true for the first time; check the wording still describes what the code now does, and correct it if it does not.
- [ ] Make the ORDER explicit in feature.md §2: the base gate — and therefore the unit's branch — comes BEFORE the step that writes the conversation material to disk. A field report hit a dead end here (the material write refused on an integration base); this run did not reproduce it, because the auto-branch hook cut the branch on that same write. Both readings are served by saying the order out loud, and the sentence costs nothing if the guard already handles it. Do NOT claim the dead end was reproduced — it was not.

## Files

- `plugin/commands/spec.md`
- `plugin/commands/feature.md`
- `plugin/refs/spec/resume-loop.md`
- `apps/rt/tests/spec_flow_prose.rs`
