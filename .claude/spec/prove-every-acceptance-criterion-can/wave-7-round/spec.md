---
id: wave.prove-every-acceptance-criterion-can.7-round
---

# wave-7-round

## Summary

A wave caches the diff of the files it declared, not the whole round's commit — so a blocked sibling's work stops leaking into a finished wave's record.

## Network

- Parent: [[spec.prove-every-acceptance-criterion-can]]

## Tasks

- [ ] `wave_done.rs:298` builds the wave's cached diff from `HEAD~1..HEAD`, the entire previous commit. The dispatch loop commits ONCE PER ROUND, so a round carrying several waves gives every one of them the same diff — the round's, not the wave's. The damage is not cosmetic: that cache feeds the retry context and the closing summary, so a wave that came back blocked leaks its half-written files into a finished sibling's record.
- [ ] Scope the diff to the files the wave DECLARED in its own `## Files` section, read through the existing reader rather than a new parser. Keep the commit range as it is — the range is not the problem, the breadth is.
- [ ] Degrade the way the current code does: any git error still yields an empty digest and never fails the wave. A wave that declared no files keeps today's behaviour rather than silently caching nothing.
- [ ] Test the round shape directly: two waves committed together, each one's cached diff naming only its own declared files and none of its sibling's.

## Files

- `apps/rt/src/commands/pipeline/wave_done.rs`
