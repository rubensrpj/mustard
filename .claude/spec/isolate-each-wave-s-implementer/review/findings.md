# Review — isolate-each-wave-s-implementer

## Verdict: rejected — 1 critical

## Guards, molds, criteria
All four registrations present for the new command; clippy clean; the hook exit posture is that event's documented protocol, not a violation; no mold applies to a `run` command; `run` output deterministic. AC-1..AC-9 each pass as written — but AC-5 is proven only in a fixture where the work unit IS the main checkout.

## CRITICAL — the way OUT reads the wrong tree
`apps/rt/src/commands/wave/wave_reclaim.rs:189` — reclaim derives the work unit from `git rev-parse --abbrev-ref HEAD` in the MAIN checkout, while the way IN correctly reads the INVOKING tree. The documented flow puts the session inside `.claude/worktrees/{base}_{slug}` during EXECUTE, so the main checkout is elsewhere. Reproduced against the built binary:

1. Main on `dev`, session in the unit worktree, agent checkout cut from the unit HEAD → `not-a-work-unit`, exit 1. Every wave-done blocks forever; AC-5's fold never happens in the real flow.
2. Main on ANOTHER unit branch, same session → `ok:true, reclaimed`, and the wave's commit landed on that other branch while the agent worktree AND its branch were pruned. The work went to the wrong branch silently and the wave was then reported complete.

That violates the spec's own metric ("completed waves whose commit never reached the work-unit branch = 0") and the fail-closed posture the module header claims. The unit must come from the invoking tree, and the merge must run where that branch is checked out.

## MAJOR — zero attribution reported as success
`wave_reclaim.rs:235` — when agent checkouts exist but none touches a declared path, the answer is `nothing-to-reclaim` with `ok:true`, and wave-done emits the completion. The recorded decision only authorises failing closed on SEVERAL matches; the no-match case strands work under a success verdict, contradicting AC-6. Realistic trigger: an implementer that creates files it did not declare — which this very wave-set reported doing.

## MINOR — exact-string path match
`wave_reclaim.rs:159` — declared paths are compared to `git diff --name-only` output with `==`, so a declared directory prefix, or a case-differing path on Windows, silently fails to attribute.
