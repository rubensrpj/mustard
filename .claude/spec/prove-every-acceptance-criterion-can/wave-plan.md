---
id: wave.prove-every-acceptance-criterion-can.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.prove-every-acceptance-criterion-can.1-rt]] | rt | — | The negative-test engine: run each criterion against the tree as it is, require it to fail, record the proof — plus the removal of the search-for-absence exemption that let a criterion read green while matching nothing. |
| 2 | [[wave.prove-every-acceptance-criterion-can.2-gate]] | gate | [[wave.prove-every-acceptance-criterion-can.1-rt]] | The proof is demanded during planning, not at the approval gesture; the approval door keeps a fail-closed backstop and stops reporting a gesture the current run never performed. |
| 3 | [[wave.prove-every-acceptance-criterion-can.3-amend]] | amend | [[wave.prove-every-acceptance-criterion-can.1-rt]] | Amending a criterion becomes an operation: it demands the same negative proof from the replacement and keeps the superseded version with the reason it was replaced. |
| 4 | [[wave.prove-every-acceptance-criterion-can.4-verdict]] | verdict | [[wave.prove-every-acceptance-criterion-can.1-rt]] | A verification run that could not attempt its criteria stops being allowed to declare a pass — reproduced live on this spec, which already carries a passing verdict with no work done. |
| 5 | [[wave.prove-every-acceptance-criterion-can.5-glossary]] | glossary | — | The coverage report stops treating an absent glossary as a coverage failure, and stops offering words the corpus never published — the same words whose abstention blocks the decline that would silence the list. |
| 6 | [[wave.prove-every-acceptance-criterion-can.6-signals]] | signals | — | Three reports stop overstating what they know: a hollow clarification marker becomes visible before the approval gesture, an approved-but-unstarted plan says what to do next, and the dependency pre-gate names what it checked. |
| 7 | [[wave.prove-every-acceptance-criterion-can.7-round]] | round | — | A wave caches the diff of the files it declared, not the whole round's commit — so a blocked sibling's work stops leaking into a finished wave's record. |
| 8 | [[wave.prove-every-acceptance-criterion-can.8-docs]] | docs | [[wave.prove-every-acceptance-criterion-can.2-gate]], [[wave.prove-every-acceptance-criterion-can.3-amend]], [[wave.prove-every-acceptance-criterion-can.4-verdict]], [[wave.prove-every-acceptance-criterion-can.7-round]] | Lock the two new commands into the published surface and teach the flow to use them — the proof at planning time, the amendment instead of a hand edit. |
