# Review (fix-loop 3) — apps/rt — verdict: approved, 0 critical

The prior CRITICAL is closed and was re-derived, not taken on trust:
`plugin/commands/close.md:21` now states the rule the code implements, and
`plugin/commands/qa.md:32` was corrected in the same shape with the file added
to the spec's `## Files`, so the boundary was extended rather than crossed.

The fix is GUARDED, not merely edited: AC-3's test gained a per-CLAUSE scan of
both rituals. Checked against the exact prior-defective text — it splits on `;`
into a clause carrying both tokens, so the previous critical would have failed
this test.

The prior MINOR ("two roots for one fact") is a non-issue: `qa_run::run`
resolves `current_dir()` FIRST and only falls back to `project_dir()`, and
`emit_qa_event` routes with that same cwd, so the recorder and
`close_orchestrate`'s reader agree on the root.

Verified independently: all four criteria pass individually; `cargo build
--workspace` green; `cargo test -p mustard-rt` 1719 + 1724 passed, 0 failed
across every target including the pre-existing fail-open integration test;
clippy zero diagnostics in either touched file.

Live with the freshly built binary — six scenes, not code presence:
no-AC spec → refused, exit 2, no events written; red-AC `--archive` → refused,
exit 2, spec dir untouched; passing spec → admitted and `pipeline.complete`
written; already-terminal spec with NO verdict → `--archive` succeeds, so the
documented hygiene sweep survives; `close-orchestrate` on a no-AC spec →
`ok:false, summary:"skip", chained:false`; on a passing spec → `ok:true,
summary:"pass", chained:true, verified:true` — which also proves the
`qa_overall` payload-path repair is live and that the gate is not simply shut in
every direction.

Guards clean: no new `run` subcommand, no unwrap/expect outside `cfg(test)`, no
verdict from an observer, `main.rs` untouched, deterministic ordered JSON out.

## MINOR (non-blocking)

1. `plugin/commands/close.md:23` — "refuses on its own with exit 2" is exact
   only when the red gate is QA; a red `review-spans` / `docs-stale-check` is not
   read by `close_admission`. FIXED in the same pass: the sentence now scopes
   itself to the QA gate and names what the other two do instead.
2. `apps/rt/src/hooks/write/close_gate.rs:410` — the legacy `PreToolUse` adapter
   still honours the retired "no AC → advisory" leniency. It can only ALLOW and
   cannot write `pipeline.complete`, so no door is open. Pre-existing and outside
   this spec's boundary — carried as the seed of a follow-up, not silently filed.
