---
id: wave.prove-every-acceptance-criterion-can.5-glossary
---

# wave-5-glossary

## Summary

The coverage report stops treating an absent glossary as a coverage failure, and stops offering words the corpus never published — the same words whose abstention blocks the decline that would silence the list.

## Network

- Parent: [[spec.prove-every-acceptance-criterion-can]]

## Tasks

- [ ] In `glossary_coverage.rs`, split ABSENT from THIN. `score()` at line 213 turns `!present` into `missing` and puts every matched term into `uncovered`, so a project with no glossary reports 0% coverage over a file nobody wrote plus a full list of terms to interrogate. Absence and thinness ask for opposite actions — one asks for a first glossary, the other for more entries — so give absence a verdict of its own and NO term list. Keep the shape of the JSON stable (the same keys, always present) because callers read it positionally.
- [ ] Stop offering words the corpus never published. Field evidence: a 21-item list carrying identifier fragments (`split201`, `completedat`, an interface-prefixed name, the package's own name). They come from the scan model's matched terms, and the module already admits its stem tier answers with fragments no human typed. A term the published index says nothing about is not vocabulary anyone can define — drop it from `uncovered`. Derive this from the corpus, never from a hand-written stopword list: this project forbids curated lists, and `group_rarity_x1024` already answers 'did the index publish this word' as `None`.
- [ ] That single change also unblocks the antidote that exists and never fires. `decline_reason` at line 341 requires a quorum, `judged * 2 >= open`, and an unpublished fragment counts toward `open` while never counting toward `judged`. So the noise that dirties the list is the very thing preventing the decline that would silence it — remove the fragments from the open set and the quorum starts reflecting words the corpus can actually judge. Verify this reasoning against the code before relying on it, and if it does not hold, say so in the wave return rather than forcing the outcome.
- [ ] Test both directions: an absent glossary yields the absent verdict with an empty term list, and a genuinely thin one still reports its open domain terms exactly as today. Then a set mixing real vocabulary with unpublished fragments offers only the real vocabulary — and the same set, when every judged word is repository-wide, reaches the decline it could not reach before.

## Files

- `apps/rt/src/commands/glossary_coverage.rs`
