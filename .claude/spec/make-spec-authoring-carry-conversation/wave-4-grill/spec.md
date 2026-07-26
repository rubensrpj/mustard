---
id: wave.make-spec-authoring-carry-conversation.4-grill
---

# wave-4-grill

## Summary

Let the grill say when it does not apply, so declining becomes a recorded outcome instead of a silent skip.

## Network

- Parent: [[spec.make-spec-authoring-carry-conversation]]
- Depends on: [[wave.make-spec-authoring-carry-conversation.1-clarify]]

## Tasks

- [ ] Run the grill against this very project to see the failure: the matched terms come back as `work`, `back`, `clean`, `tree`, `git`, `base`, plus a truncated `waveli`. The check was designed for a business domain, where a term like `payable` has a definition worth capturing. In a harness, the domain vocabulary IS technical vocabulary, so the matcher returns generic words and the grill would ask low-value questions.
- [ ] Add a verdict for exactly that: the terms matched but they are not domain vocabulary. It sits beside the existing verdicts and is not an error — declining is a legitimate outcome, and the point is that it becomes VISIBLE rather than a skip nobody records.
- [ ] Decide the signal deterministically, from the corpus, never from a hand-curated stopword list — this project has a standing rule against curated lists precisely because they rot and encode one person's taste. Derive it from what the scan model already knows: a term that is ubiquitous across the repository carries no domain meaning; a term concentrated in a few places does.
- [ ] Wire the decline to wave 1's marker: a declined grill is a stated reason, which is exactly the substance the clarification gate now accepts. That closes the loop — the orchestrator can no longer skip in silence, because the honest path is one command away and the dishonest one is refused.
- [ ] Fail open, as this command already does: when the scan model is unavailable the verdict stays the existing not-applicable value and nothing changes. A coverage check must never become a new failure mode.
- [ ] Test `grill_declines_when_terms_are_not_domain_vocabulary`: a corpus whose matched terms are ubiquitous yields the declining verdict with a stated reason; a corpus with genuine concentrated domain terms still yields the ordinary verdicts. Both directions, because a decline that fires always is as useless as one that never fires.

## Files

- `apps/rt/src/commands/glossary_coverage.rs`
