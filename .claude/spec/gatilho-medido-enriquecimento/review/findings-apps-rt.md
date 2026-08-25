## Verdict: APPROVED

**Guards (apps/rt) — all pass**
- *No panic / fail-open*: `enrichment_gap.rs` has zero `unwrap`/`expect` outside `#[cfg(test)]`; every failure path (no census, unreadable dir, unparseable model) returns an empty gap. `cargo clippy -p mustard-rt --all-targets` → `0 errors, 144 warnings` (all pre-existing; the new hex literal warns exactly like its 17 siblings at `project_seed.rs:809-828`).
- *Four registrations for a new `run` subcommand*: N/A — no subcommand was added (the stated decision). `tests/run_command_surface.rs` and `tests/template_parity.rs` untouched and green.
- *`run` stdout deterministic/byte-stable*: verified live, not by reading. In a throwaway project the notice lands on stderr and stdout stays the single JSON line:
  - stdout: `{"ok":true,"kind":"pipeline.kind","spec":"demo","branch":"feature/demo","type":"feature","typeFrom":"explicit"}`
  - stderr: `base-gate: enrichment stale — 1 subproject on the pending ## Guards scaffold (apps/sub) and 1 mold with no author (api-service); … dispatch it once the current unit closes`
- *Observers / `main.rs` / stdin faces*: untouched.

**Effectiveness — driven, not inferred** (all in `/tmp/tmp.TyiLJF0WZW`, never in the repo)
- Fires: seeded `<!-- mustard:guards pending -->` + a model proposing `api-service` → the line above, exit 0.
- Silent: authored `apps/api/.claude/skills/api-service-pattern/SKILL.md` and curated the Guards block → `stderr bytes: 0`, stdout unchanged. Both directions confirmed.
- The signal is real for this repo today: `run scan-patterns-list` → 6 unauthored molds (`core-entry, core-outcome, dashboard-section, rt-azure, rt-branch, rt-pr`), 0 pending Guards. So the notice is not dead code here.
- Cost per pipeline open, measured: `scan-guards-list` 7 ms, `scan-patterns-list` 48 ms on the real tree.

**Claim-by-claim**
- T1/T2 — `measure` is pure, `report_if_stale` is the only effect, called in the `BaseVerdict::Open` arm at `apps/rt/src/commands/event/emit_pipeline.rs:516`, after the census refresh. It reuses the two existing traversals (`scan_guards::list::collect_pending`, `scan_patterns::list::collect`) — no third walk. Confirmed `collect` really excludes declined slugs (`list.rs:670`) and molds already on disk (`list.rs:927-938`), so the doc claim is not a boast. `default_model_path` and `collect_inner`'s hardcoded path resolve to the same file, so the presence guard and the reader cannot disagree. 3/3 unit tests pass.
- T3 — comment at `base_gate.rs:213-216` no longer sends the reader to the sealed `/scan` door.
- T4 — fingerprint recomputed independently rather than trusted: FNV-1a/64 over the CRLF-normalised template at `c3a3a010` = `0x56c5942670aa83aa`, byte-identical to the value appended at `project_seed.rs:832`. `the_fingerprint_catalog_covers_every_history` passes. The delivered `.claude/mustard/orchestrator.md` diffs IDENTICAL against the current template, so the re-seed really happened. Rule sits between `## Locating code` (l.32) and `## Efficiency` (l.38); template is 6388 chars, well under the 10000 injection cap.
- T5 — `plugin/commands/scan.md` description now names the measured trigger instead of "visibly stale".
- T6 — checklist box says unchecked, but it is done and green. `cargo test --test plugin_prose_matches_shipped_behaviour the_router_prose_names_the_signal_the_gate_emits` → `ok`. It asserts prose against the compiled `ENRICHMENT_STALE_TAG`, not a copy. Only the box is stale.
- CHANGE REQUESTS — the single entry is "segue"; nothing substantive to drop.

**Full suite**: `cargo test --workspace` → `3023 passed, 0 failed, 6 ignored (78 suites, 64s)`.

**Mold contract**: `rt-outcome-pattern` is the only skill whose `paths` cover `commands/event/**`. `EnrichmentGap` carries no `Outcome` affix and is not a command core's non-exiting return (there is no CLI face, by design), so the mold does not bind. It nonetheless matches the shape the mold prescribes — bundle struct, no serde, separate `gap_line` renderer, explicit `&Path` root, no `process::exit`. No deviation to report.

**Minor, non-blocking**
- `enrichment_gap.rs:70,74` — fields are `pub(crate)` but only the module reads them; "visibility tracks the caller" would make them private.
- `plugin_prose_matches_shipped_behaviour.rs:1930` — half 4 is `emit.contains("enrichment_gap::report_if_stale")`; it would still pass if the call drifted into the `Abstain` arm. Weak lock on placement.
- `9bd2d821` swept the operator's pre-existing uncommitted `mustard.json` stamp (0.1.37→0.1.41) into a commit; it is a labelled chore commit apart from the two feature commits, which is the flow's own pattern.

**Repository state**: `git status --porcelain` is empty — exactly as the implementer left it. Every experiment ran in `/tmp/tmp.TyiLJF0WZW`; the only writes inside the repo were `target/` build artefacts.

<VERDICT>{"verdict":"approved","critical":0,"findings":[{"severity":"minor","location":"apps/rt/src/commands/event/enrichment_gap.rs:70","summary":"EnrichmentGap fields are pub(crate) though only the declaring module reads them; private would match the visibility-tracks-the-caller convention"},{"severity":"minor","location":"apps/rt/tests/plugin_prose_matches_shipped_behaviour.rs:1930","summary":"substring assertion on the report_if_stale call would still pass if it drifted into the Abstain arm"},{"severity":"minor","location":"mustard.json:29","summary":"operator's pre-existing uncommitted version stamp was swept into chore commit 9bd2d821"}]}</VERDICT>
