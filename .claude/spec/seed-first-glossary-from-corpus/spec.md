---
id: spec.seed-first-glossary-from-corpus
---

# Seed the first glossary from the corpus: a project with no CONTEXT.md gets a short list of terms worth defining instead of a blank page

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Before a plan of any size is approved, this harness asks whether the words the
request uses have been settled — and it looks for the answer in a domain
glossary the project is supposed to keep. That check is sound. What nobody
noticed is that nothing in the harness ever creates the glossary it presupposes,
and this repository does not have one.

So every project meets the same wall on its first sizeable plan, and the only
way through is the escape route: record a reason for why no clarification
applied, and move on. A route taken every single time stops being an escape
route. It becomes the path — and a gate everyone walks around stops telling
anything apart, which is the failure the gate exists to prevent.

A previous change already stopped the report from misdescribing the absence: a
project with no glossary is now told exactly that, and is handed no list of
supposedly thin entries, because "which entries are thin" is a question with no
meaning when there is no file. That was right, and it left the operator with a
correct message and a blank page.

The remaining move is to answer the other question — not "which entries are
thin" but "which words would a first glossary be worth opening with". The
repository can answer that about itself: its own index already reports how
concentrated each word is, and that arithmetic is already used here to prove the
opposite point, that a word said everywhere is not worth defining. Read from the
other end, the same numbers name the handful of words that are.

## Users/Stakeholders

The operator meeting the clarification gate on a project's first real plan, who
today is told to settle the vocabulary and given nothing to settle it with. And
the gate itself, whose credibility depends on the ordinary answer being to
satisfy it rather than to declare it inapplicable.

## Success Metric

A project with no glossary gets a short, corpus-derived list of terms worth
defining first, so starting one costs a conversation rather than a blank page —
and a project whose vocabulary the corpus judges ordinary gets an empty list
rather than a padded one.

## Non-Goals

Authoring what a word means: nothing here decides a definition, because a
definition invented by the tool is confabulated provenance, which this project
removed once already. Writing the file: the existing capture already creates it
at the requested destination. Forcing a glossary on anyone — the stated-reason
route stays open for a request that does not warrant one. And changing what the
coverage answer means for a project that already keeps a glossary.

## Acceptance Criteria

- **AC-1** — when a project has no glossary at all, then the report hands back a short list of terms worth defining first, drawn from the words the corpus reports as most concentrated.
  Command: `cargo test -p mustard-rt --lib glossary_coverage::tests::a_project_with_no_glossary_is_handed_a_seed`
  Expect: `[1-9][0-9]* passed`
- **AC-2** — when the corpus reports every term the request touches as repository-wide vocabulary, or publishes nothing about them, then the seed comes back empty instead of padded with noise.
  Command: `cargo test -p mustard-rt --lib glossary_coverage::tests::a_seed_is_empty_rather_than_padded_with_noise`
  Expect: `[1-9][0-9]* passed`
- **AC-3** — when a glossary already exists, then no seed is offered and the thin-coverage answer is unchanged, so the two questions stay apart.
  Command: `cargo test -p mustard-rt --lib glossary_coverage::tests::an_authored_glossary_is_never_offered_a_seed`
  Expect: `[1-9][0-9]* passed`
- **AC-4** — when the whole workspace is built, then it compiles green.
  Command: `cargo build --workspace`

## Files

- `apps/rt/src/commands/glossary_coverage.rs`
- `plugin/refs/feature/glossary-grill.md`

## Root cause

Nothing in the harness creates a glossary, and the clarification gate
presupposes one. The previous unit stopped the report from lying about the
absence — a project with no glossary now correctly hands back an empty list of
open terms, because "which authored entries are thin" is a question with no
meaning when there is no file. But that left the operator told the glossary is
missing and handed nothing to start it with, so the only route out stayed the
escape hatch: record a reason and move on. An escape hatch taken every time is
not an escape hatch; it is the path, and a gate everyone routes around stops
discriminating anything.

## Plan

Answer the OTHER question, with the numbers already in the module:

- on the missing verdict only, publish a `seed`: the terms this request touches
  that the repository's own index reports as most concentrated, read through the
  rarity already derived here for the opposite purpose;
- keep `uncovered` empty there — the two questions stay apart, exactly as the
  previous unit separated them;
- offer nothing rather than noise: a word the index never published is not in
  the seed, and a request touching only repository-wide vocabulary gets an empty
  one;
- never author a definition. The seed is a list of TERMS to ask a human about;
  the existing capture writes the answer, and it already creates the file.

## Limits

This makes the first glossary cheap to start, not automatic. Nothing here
decides what a word means — a definition invented by the tool is confabulated
provenance, which this project removed once already. Whether the glossary is
worth having at all remains the operator's call, and the stated-reason route
stays open for a request that does not warrant one.

One criterion's negative proof is weaker than the others, and it is named rather
than glossed. AC-1 and AC-2 come back red with the feature switched off — I
checked by switching it off. AC-3 does not, and cannot: it asserts that an
already-authored glossary is offered NOTHING, which is trivially true in a world
with no seed at all. It is a guard, not a discriminator — what it actually
protects against is the obvious wrong implementation, offering the seed on a
thin glossary too, and it does fail against that. Recording it as red alongside
the other two would overstate what it proves.

## Definitions

- **seed** — the short list of terms a FIRST glossary would be worth opening with, for a project that has none: the words this request touches that the repository's own index reports as most concentrated. It is a list to ask the human about, never a list of definitions — nothing in the harness may author what a word means.
- **concentrated term** — a word the corpus publishes in few places rather than everywhere. Measured by the rarity already derived in this module (specificity divided by count), which is the corpus's own arithmetic and not a curated list.

## Decisions

- The glossary lives at CONTEXT.md in the repository root.
  Reason: The resolver already treats that name as the default and `context-slice` already reads it, so no new convention is invented — only stated. A single root file also matches how this repository already keeps CLAUDE.md, and the CONTEXT-MAP indirection stays available for a project that outgrows one file.
- Nothing in the harness writes a definition. The seed is a list of TERMS to ask about; the human answers and the existing capture writes.
  Reason: A definition invented by the tool is confabulated provenance — the exact defect this project removed once already. And `grill-capture` already creates the file at the requested path when it does not exist, so the write half needs nothing new.
- The seed is derived from the corpus, upper quartile of published rarity, never from a hand-written list.
  Reason: Project law forbids hand-curated lists, and the same statistic is already computed here for the opposite purpose: `decline_reason` uses it to prove a term is repository-wide vocabulary. Reading it from the other end costs no new machinery and cannot rot.
- The seed is published ONLY on the missing verdict, and `uncovered` stays empty there.
  Reason: Those answer different questions and the previous unit deliberately separated them: `uncovered` means 'which authored entries are thin', which is meaningless with no file. Filling `uncovered` again to carry the seed would undo that separation and re-teach the caller to read an absence as a coverage failure.
- A project whose corpus judges nothing concentrated gets an EMPTY seed, and the flow says so.
  Reason: Better to offer nothing than to offer noise: an empty seed means the request touches only repository-wide vocabulary, which is the same reading the decline verdict already publishes. Padding the list to look helpful is the theatre that teaches an operator to skip the step.

## Evidence

- Nothing in the harness creates a glossary: no file under the scan crate mentions CONTEXT at all, and no CONTEXT.md exists anywhere in this repository.
  Evidence: `apps/rt/src/commands/glossary_coverage.rs:1`
- With no glossary the report now publishes an empty uncovered list by design, so the operator is told the glossary is missing and handed nothing to start it with — the gap this closes.
  Evidence: `apps/rt/src/commands/glossary_coverage.rs:213`
- The rarity statistic and the corpus cut already exist in this module and are already used to decide the opposite question, so the seed reads the same numbers from the other end.
  Evidence: `apps/rt/src/commands/glossary_coverage.rs:267`
- group_rarity_x1024 answers None for a word the index never published, which is what keeps stem fragments out of the seed for free.
  Evidence: `apps/rt/src/commands/glossary_coverage.rs:290`
- grill-capture already creates the destination file when none exists, falling back to the first requested --context path, so the write half of the bootstrap needs no new code.
  Evidence: `apps/rt/src/commands/grill_capture.rs:56`
- contextFile already names a concrete destination even when nothing resolved on disk, so the flow has somewhere to point the capture at.
  Evidence: `apps/rt/src/commands/glossary_coverage.rs:417`
- The shipped grill instruction already tells the reader to offer a first glossary under the missing verdict and to name the terms from the intent and the digest anchors rather than from uncovered — prose that currently asks the reader to do by eye what the corpus can answer.
  Evidence: `plugin/refs/feature/glossary-grill.md:30`