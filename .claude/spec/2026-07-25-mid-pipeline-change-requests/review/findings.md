# Review — 2026-07-25-mid-pipeline-change-requests

## Verdict: approved (0 critical, 1 major, 4 minor)

## Verified, not taken on trust
- AC-1: passes, and the chain is real — the test goes through the same render fn the command face calls, and the log reader keeps only bullet lines, which is the shape written.
- AC-2: passes, two-sided (blank refuses AND a real instruction still records).
- AC-3: `cargo build --workspace` finished clean.
- Live CLI in a temp project, beyond the inner fn the ACs exercise: a real instruction recorded to both files with whitespace collapsed and the stage tag carried; a blank one refused with the spec dir untouched; an unknown slug refused. The dispatch arm is genuinely wired.
- Guards: all four registrations present; the surface permutation test passes; clippy emits nothing touching the new file; no unwrap/expect outside tests; the run face does not read stdin. No mold applies.

## MAJOR — the whitelist justification is already stale
`apps/rt/tests/template_parity.rs:37` defers the naming prose to the parent spec's documentation wave, but that wave finished in the IMMEDIATELY PRECEDING commit. No textual caller exists anywhere under the plugin. The command ships as dark surface with an escape hatch pointing at a wave that can no longer drop the row. Not blocking — the spec's Boundaries put the prose out of scope and the Guard sanctions a JUSTIFIED row — but it needs an owner, or the ratchet never fires.

## MINOR
- Records into a terminal (Completed) spec — verified live — while the observer twin is fail-CLOSED on non-Active, because post-close is the amendment window's territory. The deliberate twin diverges from its own sibling.
- The "did it land" proof is substring containment, so a repeated identical instruction whose append silently failed still reports success. Weaker than the module doc's claim that a loss is reported rather than assumed away.
- The report emits an absolute machine path, in tension with the byte-stability guard. Downgraded: two sibling commands do the same and no snapshot covers this one.
- A refusal exits 0 while sibling spec-family commands exit non-zero on error, so a shell caller chaining on && reads a refusal as success.

## Observation (not a finding)
One full-suite run reported a single failure, but that same invocation also failed to remove the binary — a concurrent cargo held it. Re-ran the full suite twice and the bins three times: all green. Treated as a build-lock race, flagged so it is not silently forgotten.
