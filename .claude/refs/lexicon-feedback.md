# Lexicon feedback — feed the self-learning cross-language dictionary

Loaded by flows that fold what a run taught the cross-language dictionary back into it, so the NEXT query lands deterministically **without an LLM**. **Single source of truth — reference this file, never copy the prose.** Used by `/close` (every close — feature + bugfix) and `/task` (no close, so it feeds here). Pure data + gated; **fail-open** — no `pt-en` pair or no candidates → skip silently.

```bash
mustard-rt run lexicon-suggest   # `candidates` (re-query bridges) + `locationCandidates` (found OUTSIDE the digest)
```

- **`candidates` `{missed, bridged}`** — a CONFIRMED bridge: a re-query in the code's own words landed. Accept each: `mustard-rt run lexicon-suggest --accept {missed}={bridged}` (gated — the code term must be a real mined term; idempotent if already covered).
- **`locationCandidates` `{missed, files}`** — a term the digest MISSED whose answer you found by other means (Glob/Grep/exploration). Open the file(s), pick the code term that names the concept, and accept it: `--accept {missed}={codeTerm}`. Accept **only** when the mapping is clear — a wrong bridge poisons every future query, so skip the unsure ones.

This is what makes the dictionary self-feed: the exact cases where the digest failed and you solved it by hand become the bridges that make it succeed next time. Runs on every close (feature + bugfix) and at the end of every `/task`.
