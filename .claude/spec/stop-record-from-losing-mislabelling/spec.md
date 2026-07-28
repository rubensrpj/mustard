---
id: spec.stop-record-from-losing-mislabelling
---

# Stop the record from losing and mislabelling: keep every wave memory with its own wave, match reality obligations by id instead of substring, spell the two unproven cases apart in the close report, teach the precheck skip in the dispatch prose, and scope the fix-loop retry context to its subproject

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Context

Stop the record from losing and mislabelling: keep every wave memory with its own wave, match reality obligations by id instead of substring, spell the two unproven cases apart in the close report, teach the precheck skip in the dispatch prose, and scope the fix-loop retry context to its subproject.

Why now. Five defects, one habit: the record states something other than what
happened. They were not found by reasoning — every one of them was measured
during a single run of the pipeline.

Five memory blocks were emitted and four files exist; the file describing a
second wave's decision carries the first wave's number in its header. An account
of one obligation silently discharges another because the comparison looks for a
fragment instead of an identifier. The close report gives the same shape to a
check that was never taken and to one that came back red. The dispatch prose
still teaches a reading the code stopped doing. And two fix-loop prompts came out
with identical task text because the context is built per spec rather than per
subproject — two writers, one set of files.

## Users/Stakeholders

The wave that reads the previous wave's memory to avoid repeating its mistake;
the reviewer six months from now, for whom an attribution is either right or
misleading with no third option; and the operator dispatching a repair, who
should not have to notice by hand that two agents were handed the same job.

## Success Metric

Every wave's memory exists and carries its own wave number; an obligation is
discharged only by its own identifier; the close report's two unproven cases read
differently; and a prompt rendered for one subproject carries only that
subproject's findings. Concretely: re-running the previous spec's rounds would
produce five memory files with five correct headers instead of four with two
wrong ones.

## Non-Goals

Redesigning how memories are captured stays out — the channel works; what fails
is which wave claims them and how many survive. Changing what a reality
obligation means stays out too: this is about matching an identifier correctly,
not about judging whether a duty was truly met, which no code can know. And the
boundary gate's advisory default stays out, as argued when it was first deferred:
its noise comes from scope lists nobody updates, so blocking today would convert
honest warnings into blocks.

## Acceptance Criteria

Each criterion names the test that proves it and demands a non-zero pass count:
a filter matching nothing exits 0 and prints "0 passed", and `[1-9][0-9]*` is
what refuses to read that as success.

- **AC-1** — when several waves of one round each emitted a memory, then each is
  written under the wave that emitted it and none is dropped
  Command: `cargo test -p mustard-rt every_wave_keeps_its_own_memory`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-2** — when a wave accounts for the obligation RO-3.10, then RO-3.1 stays
  unaccounted instead of being discharged by the substring
  Command: `cargo test -p mustard-rt obligation_match_is_by_id_not_substring`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-3** — when the close report names an unproven criterion, then a check never
  taken and one that came back red read differently
  Command: `cargo test -p mustard-rt close_report_spells_the_two_unproven_cases_apart`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-4** — when a fix-loop prompt is rendered for one subproject, then it carries
  that subproject's findings and not another's
  Command: `cargo test -p mustard-rt retry_context_is_scoped_to_its_subproject`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-5** — when the dispatch prose tells the reader how to read a precheck, then
  it teaches the skip marker beside the ok reading
  Command: `cargo test -p mustard-rt dispatch_prose_teaches_the_precheck_skip`
  Expect: `ok\. [1-9][0-9]* passed`
- **AC-7** — the project build passes green
  Command: `cargo build --workspace`

## Checklist

- [ ] T1 — Every wave keeps its own memory: the capture is scoped to the wave that
      emitted it, and no memory is dropped when several waves close in one round.
- [ ] T2 — The obligation match compares identifiers, not fragments.
- [ ] T3 — The close report gives its two unproven cases distinct wordings, as the
      module's own doc already requires.
- [ ] T4 — The retry context is assembled per subproject, so two prompts rendered
      for different subprojects never carry the same task text.
- [ ] T5 — The dispatch prose teaches the precheck skip marker beside the ok
      reading.
- [x] T6 — DROPPED, not done. The premise was false: the operator deleted
      MUSTARD-COMMANDS.md and install-retrieval.ps1 by hand, by mistake. No
      mechanism removed them, so there is nothing to stop and no invariant worth
      locking. Both restorations were correct and the files are tracked and
      byte-exact again. Three suspects had been refuted by verification before the
      real cause was simply stated — the answer was one question away the whole
      time, and nobody asked it.

## Definitions

- **process memory** — the per-wave notes wave-done writes under <spec>/memory/, which steer the waves that follow
- **accounted duty** — a reality obligation whose id appears in what the wave recorded — the fact wave-done can actually check
- **retry context** — the findings block the renderer composes into a fix-loop prompt

## Decisions

- every wave keeps its own memories, and none are dropped
  Reason: in the run that shipped this, five memories were emitted and four files written: the first wave-done of each round swept the pending ones, stamped them with its own number and kept exactly two — so a wave's decision was attributed to a sibling and one was lost outright
- the obligation match compares ids, not substrings
  Reason: an account of RO-3.10 currently clears RO-3.1, which turns a report of one duty into a silent discharge of another
- the retry context is scoped to the subproject it is rendered for
  Reason: two fix-loop prompts rendered for different subprojects came out with identical task text, sent two agents into the same files, and only the agents' own care kept the work from being written twice
- the close report spells its two unproven cases differently
  Reason: the module's own doc says the two must never read alike, and today only the ledger separates them

## Evidence

- The reality-obligation match is a bare substring test, so an account naming RO-3.10 also clears RO-3.1 without anyone reporting it.
  Evidence: `apps/rt/src/commands/pipeline/wave_done.rs:130`
- Measured on the run that shipped: agents emitted five memory blocks and the memory directory holds four files. Only the first wave-done of each round wrote, it wrote exactly two, and it stamped a sibling wave's content with its own number — the file naming a wave-2 decision carries wave: 1 in its header.
  Evidence: `apps/rt/src/commands/pipeline/wave_done.rs:1`
- The close report collapses two different verdicts into one shape: a criterion whose confirmation was never taken and one that came back red are both emitted as a bare id under the same key, while the module doc insists the two must never be spelled alike.
  Evidence: `apps/rt/src/commands/pipeline/close_pipeline.rs:154`
- The dispatch prose still instructs the reader to treat an ok:true precheck as clearance, so the skip marker that now rides the trim has no documented reader.
  Evidence: `plugin/refs/spec/resume-loop.md:64`
- The fix-loop retry context is assembled per spec rather than per subproject, so two prompts rendered for different subprojects carry the same task text and send two writers into one set of files.
  Evidence: `apps/rt/src/commands/agent/render/retry.rs:1`