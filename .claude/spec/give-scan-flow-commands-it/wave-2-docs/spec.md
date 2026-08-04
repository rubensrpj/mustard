---
id: wave.give-scan-flow-commands-it.2-docs
---

# wave-2-docs

## Summary

Re-aim what the flow TEACHES: stop promising the channel is size-safe, stop forbidding the file the harness itself writes, permit N relay calls at block boundaries, and document how a truncated return converges.

## Network

- Parent: [[spec.give-scan-flow-commands-it]]
- Depends on: [[wave.give-scan-flow-commands-it.1-commands]]

## Tasks

- [ ] In apps/rt/src/commands/agent/render/role.rs, correct the sentence that tells the patterns agent to `deliver every mold in one message and never worry about its size`. The truth is two-sided and both halves matter: the RELAY does not care about size, but the CHANNEL can truncate — and it truncates from the FRONT, so the preamble and the earliest blocks are what is lost. Keep it short; this is a prompt, not a manual. Touch ONLY the prompt text — wave 1 owns everything else in this file.
- [ ] PRESERVE THE SINGLE-BLOCK PARAGRAPH. `template_parity` runs a REVERSE ratchet: it fails any registered command that no prose calls. `scan-patterns-apply` and `scan-patterns-decline` are named ONLY in the `Single-block face` paragraph of step 4. Rewriting step 4 without carrying that paragraph forward orphans both commands and turns this text edit into a red test. Verify with `cargo test -p mustard-rt --test template_parity` before reporting done.
- [ ] Rewrite step 4 of plugin/commands/scan.md. Remove `never via a temp file` — the harness writes that file itself, so forbidding it forbade the only clean path. Teach the file face instead: a return that came back as a path is forwarded with `--content @<path>`, and the reader accepts both raw text and the harness's JSON.
- [ ] Replace `one call per agent, never one per block` with the invariant that is actually true. Forbid what is genuinely dangerous — splitting text INSIDE a block, with a regex or a loop. Permit N relay calls split at `=== END ===` boundaries, and say WHY it is safe: the relay is idempotent per block and its report is additive. Name the reason the split is sometimes forced: a single process argument on Windows caps near 32767 characters, so a return that arrives only in the orchestrator's context cannot always be one call.
- [ ] Add a re-dispatch convergence paragraph. A truncated return is not a total loss: apply the intact blocks, record the declines, re-render. Created molds and declined slugs leave the worklist, so the next round is strictly smaller. Use the measured run: 59 clusters -> 5 molds + 5 declines saved -> 49 clusters -> the second round came back persisted and intact. Name the direction of the cut — the front is lost, the tail survives — because that is what makes the recovery predictable.
- [ ] Re-aim the script ban in the Inviolable section rather than lifting it. A script is still a SYMPTOM; what changes is that the two cases that used to force one — a persisted return and a convergence count — now have commands. Keep the rule, correct its target.
- [ ] Update MUSTARD-COMMANDS.md so the published scan-patterns-* surface carries the new flags.

## Files

- `apps/rt/src/commands/agent/render/role.rs`
- `plugin/commands/scan.md`
- `MUSTARD-COMMANDS.md`
