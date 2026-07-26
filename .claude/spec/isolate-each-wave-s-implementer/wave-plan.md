---
id: wave.isolate-each-wave-s-implementer.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.isolate-each-wave-s-implementer.1-rt]] | rt | — | The way IN: an agent worktree is cut from the work unit's HEAD; without a unit, from the flow's primary base — never the remote's default; and never at all while the tree holds uncommitted code. |
| 2 | [[wave.isolate-each-wave-s-implementer.2-plugin]] | plugin | — | Create the implementer subagent the plugin never had — deliberately thin, carrying only what a prompt cannot carry: the worktree isolation. |
| 3 | [[wave.isolate-each-wave-s-implementer.3-rt]] | rt | [[wave.isolate-each-wave-s-implementer.1-rt]] | The way OUT: a new wave-reclaim step that folds a finished wave's commit back onto the work-unit branch, and refuses to report the wave complete when it cannot. |
| 4 | [[wave.isolate-each-wave-s-implementer.4-rt]] | rt | [[wave.isolate-each-wave-s-implementer.2-plugin]], [[wave.isolate-each-wave-s-implementer.3-rt]] | The switch: point every writing role at the isolated implementer — last, because both the way in and the way out must already work. |
| 5 | [[wave.isolate-each-wave-s-implementer.5-docs]] | docs | [[wave.isolate-each-wave-s-implementer.4-rt]] | Correct the role-to-subagent map wherever it is written down, and pin it to the code so prose can never contradict behaviour again. |
