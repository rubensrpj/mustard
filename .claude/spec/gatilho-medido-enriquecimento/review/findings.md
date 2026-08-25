All checks complete. Repository left exactly as found (`git status` clean, no config touched); every experiment ran in `/tmp/tmp.wVGD38HOP6`.

## Verdict per claim

**Guards (`apps/rt/CLAUDE.md`) — PASS**
- No new `run` subcommand → the four-registration guard does not apply (grep on `scan_cli.rs`/`commands/*/cli.rs`: no new variant).
- `run` stdout stays byte-stable. Live proof in a throwaway repo: `emit-pipeline --kind pipeline.kind` printed exactly 1 stdout line `{"ok":true,"kind":"pipeline.kind",...}` and the notice went to stderr only.
- No `unwrap/expect` outside `#[cfg(test)]`; `cargo clippy -p mustard-rt --all-targets` emits zero warnings mentioning `enrichment_gap`.

**Mold contract — PASS.** The only mold whose `paths:` covers `apps/rt/src/commands/event/**` is `rt-outcome-pattern`. `enrichment_gap.rs` follows it: bundle struct with `pub(crate)` fields, no serde derive, renderer as a separate free fn (`gap_line`), producing fn takes an explicit `&Path`, no `process::exit`, doc states the in-process caller. `rt-report-pattern`/`rt-item-pattern`/`rt-verdict-pattern`/`rt-gate-pattern` do not list this folder.

**AC-1..AC-5 — all PASS, run verbatim (via `rtk proxy` so cargo output is unfiltered)**
- AC-1/2/3: the three `enrichment_gap::tests::*` matched `test result: ok. 1 passed`.
- AC-4: `the_router_prose_names_the_signal_the_gate_emits` matched `test result: ok. 1 passed`.
- AC-5: `cargo build --workspace` → `Finished`, rc=0. Full suites: `mustard-rt` 2141 passed (39 suites), `mustard-core` 665 passed.

**Effectiveness, not code presence — PASS (live, adversarial)**
- Gap present → stderr: `base-gate: enrichment stale — 1 subproject on the pending ## Guards scaffold (apps/api) and 1 mold with no author (api-service); … dispatch it once the current unit closes`.
- Gap closed (mold authored on disk + Guards curated) → stderr empty, stdout unchanged.
- Non-opening kind (`pipeline.scope`) → silent.
- Doc claim "already excluding a mold present on disk and a slug the agent declined" independently confirmed at `scan_patterns/list.rs:670` (`declined`) and `:677` (`mold_present` → drop `mold_exists`) — not taken on the implementer's word.

**T4 seed/fingerprint — PASS.** `diff` of `packages/core/templates/mustard/orchestrator.md` vs `.claude/mustard/orchestrator.md`: identical (6388 bytes each). The ratchet `the_fingerprint_catalog_covers_every_history` walks real git history and passes, so `0x56c5942670aa83aa` is the genuine superseded hash.

**CHANGE REQUESTS — PASS.** The only entry is `segue` (proceed); nothing substantive was dropped.

## Findings

1. **MAJOR — `.claude/spec/gatilho-medido-enriquecimento/spec.md:82`.** T6 is left `- [ ]` although the work IS delivered (the parity test exists and passes). The parent `## Checklist` is the first thing `close_gates::find_unmarked_checklist` reads (`close_gates.rs:389`), so this blocks CLOSE. Reproduced in the sandbox with the same shape:
   `[Close Gate] checklist has 1 unmarked item(s) … - T6 — the parity test` → exit 1.
   Fix is one command (`mark-checklist-item`), not a code change.

2. **MINOR — `apps/rt/src/commands/event/enrichment_gap.rs:1`.** Module doc says the gap is said "once"; it is emitted on every `pipeline.kind` opening until the gap closes. On this very repository the line will fire now (measured: `scan-patterns-list` returns 6 unauthored candidates — `dashboard-section, rt-pr, rt-azure, rt-branch, core-entry, core-outcome`; `scan-guards-list` returns `[]`), which is the documented decision, but "once" reads as per-session and is not what the code does.

No Guards violation, no mold violation, no correctness defect. Approved.

<VERDICT>{"verdict":"approved","critical":0,"findings":[{"severity":"major","location":".claude/spec/gatilho-medido-enriquecimento/spec.md:82","summary":"T6 left unchecked though delivered; the close gate refuses the Close transition on an unmarked checklist item (reproduced: exit 1)"},{"severity":"minor","location":"apps/rt/src/commands/event/enrichment_gap.rs:1","summary":"module doc says the notice is said 'once' but it is emitted on every pipeline-opening emit until the gap closes"}]}</VERDICT>
