# Tactical Fix: Mid-pipeline change requests record the user's words, not the change

## Context

Tactical fix derived from [[make-spec-authoring-carry-conversation]], found while implementing it.

**Today.** While a spec is active, an observer captures every user prompt into the spec's change log, and the per-wave renderer feeds those bullets to the agents. The capture is automatic and blind: it stores the sentence the user typed.

**Why that is a problem.** A reply carries its meaning from the conversation, not from its own words. In the run that produced the parent spec, an approval was recorded as nothing more than a timestamp, a stage and the four words the user typed — an approval with no trace of what was approved. An agent reading it learns nothing it can act on. It is the same defect the parent spec removes from spec authoring, where the conversation produces the meaning and the record keeps only the words, surviving here in the other entry point, which nobody had examined.

**Why the observer is not the place to fix it.** It fires on the prompt event and sees only the sentence; it cannot know what the sentence approves. The one who knows is the orchestrator. So the blind capture stays exactly as it is — a useful, greppable trail — and what is missing is a way for the orchestrator to record the instruction alongside it.

**How it was worked around.** By hand, appending a bullet to the change log in the precise shape the renderer filters for. That worked and reached the prompt, but hand-writing a spec file in the exact format a reader expects is the same fragility the parent spec describes for amendments: one wrong prefix and the instruction silently never arrives, with nothing reporting the loss.

## Acceptance Criteria

- **AC-1** — when the orchestrator registers a change request with an explicit instruction, then it lands in the spec's change log in the shape the per-wave renderer reads, and reaches the next rendered prompt — without anyone hand-formatting a bullet.
  Command: `cargo test -p mustard-rt change_request_instruction_reaches_the_next_prompt`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-2** — when the command is given an empty or whitespace-only instruction, then it refuses and writes nothing, so the log cannot fill with entries that say nothing.
  Command: `cargo test -p mustard-rt change_request_refuses_an_empty_instruction`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-3** — the workspace builds green.
  Command: `cargo build --workspace`

## Files

- `apps/rt/src/commands/spec/change_request.rs` (new) — the command that appends a structured instruction
- `apps/rt/src/commands/spec/cli.rs` — the two registrations the crate's guard requires: the enum variant and the dispatch arm
- `apps/rt/src/commands/spec/mod.rs` — module registration
- `apps/rt/tests/run_command_surface.rs` — the locked list of published subcommands

## Boundaries

IN: the files above and their tests.

OUT: the blind prompt capture, which stays as it is — it is the raw trail, and this fix adds the deliberate record beside it, never replacing it. Also out: the flow prose that will tell the orchestrator to use it; that belongs to the parent spec's documentation wave, which is already running.

<!-- wikilinks-footer-start -->
- [make-spec-authoring-carry-conversation](?) ⚠ unresolved
<!-- wikilinks-footer-end -->