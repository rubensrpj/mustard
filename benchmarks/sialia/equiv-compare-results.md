# Equivalence sources on the ungated pagerank (C2 shape) — zero-Claude generation

Generated 2026-07-08 20:52 by `compare-equiv.ps1`. Model+dict: the post-revert scan (raw PT dictionary back; scan wall-time ~27 s on sialia). Query = raw `pt` + the source's added EN tokens; `grain rank --direct-base 100000`; scored n=13, target-OR-secondary.

| Equivalence source | Acc@5 | Acc@10 | hit ids @5 |
|---|---|---|---|
| none (raw PT) | 5/13 | 7/13 | 1,4,5,6,12 |
| claude (the bar) | 6/13 | 7/13 | 1,3,4,5,6,12 |
| mt (local Marian) | 6/13 | 7/13 | 1,3,4,5,6,12 |
| cooc (no model) | 4/13 | 7/13 | 1,4,5,6 |

Bars: claude-authored equivalences on the pre-revert pair measured 6/13 @5, 7/13 @10.

