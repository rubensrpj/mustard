# Third review — isolate-each-wave-s-implementer

## Verdict: rejected — 2 critical

All nine ACs pass, 3471 tests green, clippy clean, Guards audited and not violated, no mold applies. **The criteria all pass. They do not cover the delivered feature end to end, and it is broken there.**

## CRITICAL 1 — the way OUT is blind to a checkout holding UNCOMMITTED work
The candidate sweep admits a checkout only when its commit count exceeds zero. A checkout holding the wave's entire output as working-tree changes is therefore never a candidate, the pool reads empty, and the command answers success. Proven against the built binary, not by reading: a unit branch plus an agent checkout containing an uncommitted file returned `{"ok":true,"action":"nothing-to-reclaim"}`, exit 0 — and `wave_done` then passed the gate and emitted the completion.

This is the exact silence the module header forbids, and it fails AC-6's own words and the spec's zero-metric. The dirty-checkout refusal is unreachable in this state: it sits AFTER the commit-count filter. The spec's own Context states the platform KEEPS a checkout that finished with changes, so this is the normal case, not an edge.

## CRITICAL 2 — nothing in the shipped flow commits inside the isolated checkout
The execution loop still assigns the per-wave commit to the orchestrator. With isolation live for every writing role, the orchestrator sits in the UNIT worktree — a different checkout from the agent's, whose slug is recorded nowhere, as this spec's own decision states. The implementer agent file only remarks that `add -A` WOULD stage the right scope; it never instructs the implementer to commit before returning, and neither does the rendered role contract nor the prompt reference.

Combined with CRITICAL 1: the first real EXECUTE round after this merge loses every wave's work silently while reporting it complete.

## MAJOR — destructive sweep widened onto a fail-open probe
The collector went from inert to sweeping every non-unit entry, and guards removal with a dirty-path probe that returns an empty vec on ANY git failure — read as "clean". A directory git can no longer status is removed after the threshold with `--apply`. The safety half of a widening must stand on a positive observation of cleanliness, as the refusal in the cut does.

## MINOR
The git-flow reference still documents the non-unit cut as "the native default cut", false since the cascade replaced it. Same class of doc drift AC-8 exists to prevent, in the file AC-8 does not pin.
