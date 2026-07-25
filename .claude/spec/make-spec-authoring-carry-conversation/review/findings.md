# Review — make-spec-authoring-carry-conversation (apps/rt)

## Verified through the REAL binary, not only unit tests
- `cargo test -p mustard-rt` → 3439 passed / 0 failed (31 suites). Build clean. Clippy: no deny errors.
- AC-1/AC-2: finalize with nothing → `nothing-to-record`, no marker written; with two terms → marker carries them. A hand-planted legacy marker + a valid user approval → `approve-spec` printed "recorded NOTHING", named the remedy, exit 1, no events emitted.
- AC-3/AC-4: `--material` produced the three sections with the file:line intact; a mistyped key produced `unknown field` and NO spec dir — fail-closed proven.
- AC-9/AC-10: a real wave close wrote exactly one memory file with wave and session in its frontmatter; the residue event was rejected; the next wave's render carried the lesson under SPEC MEMORY.
- Guards and molds: no violation. All four registrations present for the new command.

## CRITICAL — the tactical fix ships inert
`apps/rt/tests/template_parity.rs:39` whitelists the new command, justified by prose that "belongs to the documentation wave". That wave had ALREADY closed when the row was written (wave 7 completed 14:10:49Z; the tactical commit is 14:24:32Z). No match for the command anywhere under `plugin/`. So nothing tells the orchestrator to call it, the deferral has no wave left to land in, and the blind four-word capture the fix exists to end keeps happening. A justified whitelist row is tolerated; this justification cannot come true.
Remedy: one line in the flow prose naming the command, then delete the whitelist row.

## MAJOR — the declined verdict never fires on this repository
`apps/rt/src/commands/glossary_coverage.rs:308`. Ran the shipped binary on four real intents, including the one that produced this spec: all returned `missing` with an empty stated reason — never `declined`. Cause, from the repo's own index (120 terms, median rarity 4132): the cut IS the median, so about half the published vocabulary sits at or above it and a single such term vetoes; and where terms are generic enough to pass, the digest emits unpublished fragments so the quorum fails (1 judged against 5 open). The module doc claims it fixed "a decline that never fires" — unsupported on the corpus it was written against. AC-5 passes only against a synthetic five-row fixture.

## MAJOR — mid-pipeline request not folded into the ACs
The 13:52 drift-guard request is implemented and passing but no AC names it, while the spec's own change log instructs exactly that. Consequence: QA never exercises the guard. Every other change request IS covered.

## MINOR
- `apps/rt/src/commands/context_cli.rs` and `apps/rt/src/hooks/observe/change_request_log.rs` appear in neither spec's Files, while both Boundaries read "IN: the files above".
- `apps/rt/src/commands/agent/context_inject.rs` — the value filter gates durable memory on three hand-written English marker lists. A non-English lesson can never qualify, and the lists are exactly the hand-curated shape the declined verdict in the same spec correctly replaced with corpus arithmetic.

## Verdict
rejected — 1 critical.
