# Review — apps/rt, spec ceremony-costs-more-than-gates (waves 1+2)

Verdict: **rejected**, 1 critical.

## AC verdicts — all five PASS

| AC | Command | Result |
|---|---|---|
| 1 | `spec_draft_materialises_the_whole_layout_in_one_call` | `ok. 1 passed` ×2 binaries |
| 1 ctl | `--lib spec_draft` | `ok. 37 passed` |
| 2 | `spec_draft_plan_refuses_an_unproven_criterion` | `ok. 1 passed` ×2 |
| 2 ctl | `--lib plan_materialize` | `ok. 7 passed` |
| 3 | `picker_approval` | `ok. 9 passed` ×2 |
| 3 ctl | `approval_marker` | `ok. 19 passed` ×2 |
| 4 | `--test spec_flow_prose` | `ok. 3 passed` |
| 5 | `build --workspace` + `test --workspace` | 4557 passed, 0 failed; clippy zero warnings |

Independently confirmed: all 10 SUPERSEDED needles existed verbatim in `b33d4264`; no surviving
copy of the old `r` contract in `plugin/` or `MUSTARD-COMMANDS.md`; the fused refusal reproduced
end-to-end on the real binary (exit 2, no `wave-plan.md`, no wave dir, draft intact). Guards:
observer returns `()`, `check: None`, no `unwrap/expect` outside tests, no new `run` subcommand,
`main.rs` untouched. Mold `rt-observer-pattern` satisfied.

## CRITICAL — the picker letter is discarded, so the marker can land on the WRONG spec

`apps/rt/src/hooks/observe/picker_approval_observer.rs:104-149`.
`is_approve_and_implement` validates the letter's SHAPE (`letter.is_ascii_alphabetic()`) and
throws its VALUE away — it returns `bool`. Attribution then falls to `active_spec()`
(session binding → current-spec → unique-pending). The picker's whole purpose is choosing a
row that is NOT the current spec.

Reproduced against `./target/debug/mustard-rt on UserPromptSubmit` on a temp project with two
pending Full plans, session `s-1` bound to `spec-x`, prompt `/mustard:spec br`:

```
---spec-x---   .approved-by-user  .events  meta.json      <- marker minted HERE
---spec-y---   meta.json
$ cat .../spec-x/.approved-by-user
spec=spec-x
via=picker
```

Harm chain: `resume_bootstrap/mod.rs:310-312` sets `approvedByUser` from bare marker presence,
and the newly-edited `resume-loop.md:20` then skips the approval presentation entirely — so
wave 1 of a plan the user never approved reaches `approve-spec`, which passes on the marker.
This defeats the exact property the spec names ("unforgeable gesture"): the gesture is real but
it is attributed to the wrong spec, which is the same thing as forging one for that spec.

Fixable in-process: `active_specs.rs:1342-1347` assigns `('a'..='z')` deterministically by row
index, so the observer can resolve letter→spec through the SAME enumerator instead of ignoring
it. No test covers a letter naming a different row.

In the observer's favour: the INVERSE case — nothing minted because attribution was ambiguous —
is safe, since §A still relays `approve-spec`, which refuses without the marker.

## MAJOR — the documented recovery after a proof refusal is a dead end

`plugin/refs/feature/full-plan.md:34` (step 4) says "Fix the criterion … and re-run the whole
call". Reproduced: re-running `spec-draft --plan` after a refusal returns
`{"ok":false,"error":"output exists; pass --force to overwrite"}`. `--force` is never mentioned,
and step 3 tells the reader "A FIRST materialisation does not come through here" — but after the
rollback there IS no layout, so `plan-materialize` is exactly what must be run. The prose
contradicts itself on the one path a refusal creates.

## MINOR

`picker_approval_observer.rs:12-15` — the module doc states in the present tense that "Today the
`r` pre-answers only the implement-now CONTINUATION", describing behaviour this same commit
removed.
