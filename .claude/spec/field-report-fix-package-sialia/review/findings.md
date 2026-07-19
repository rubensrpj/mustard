# Review — apps/rt

Verdict at review time: FAIL (1 blocking, AC-6). Blocker fixed post-review + verified → now clean.

Per-AC: AC-1 PASS (2 passed), AC-2 PASS (4 passed), AC-3 PASS (6 passed — vacuous green genuinely dead), AC-4 PASS (4 passed), AC-5 PASS (full chain + submodule/worktree discrimination via git-common-dir), AC-6 WAS FAIL (template_budget: full-plan.md 1656 + resume-loop.md 1515 > 1500-word cap, regression from wave-6 prose), AC-7 PASS (1).

Mold contract: rt-observer/rt-gate/rt-inject all respected; no new molded-kind modules. apps/rt Guards clean (no unwrap/expect outside cfg(test); the two plugin.json .expect() are inside a cfg(test) drift-guard; byte-stable run output).

Minor (non-blocking): AC-1's command filter exercises resolve-without-binding but not fail-closed-on-2+; that case IS covered by two_pending_full_plans_stay_none (green), just outside the AC substring — AC-coverage narrowness, no defect.

RESOLUTION (post-review): templates trimmed under cap; template_budget test → 2 passed. AC-6 now green.
