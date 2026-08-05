# Review — apps/rt

## Verdict: APPROVED — 0 blocking findings

**Guards (apps/rt) — all held.** No panic path added; `finding_collect`/`mark_finding` degrade via `unwrap_or_default`/`let-else`/`ok()`; `cargo clippy --workspace --all-targets` exits 0 with **zero** warnings in the five touched files. The FOUR registrations exist for both new commands (enum variant + dispatch arm in `review/cli.rs` and `spec/cli.rs`, the locked list in `run_command_surface.rs`, a justified `RUNTIME_WHITELIST` row) — proved by `every_declared_command_keeps_its_help_slot`, `forward_every_instructed_run_name_is_registered`, `reverse_every_registered_name_has_a_caller_or_a_justification`, `runtime_whitelist_stays_sorted_live_and_not_redundant`, all `... ok`. No observer returns a verdict; `main.rs` untouched; `run finding-collect` output is sorted, timestamp-free and path-free.

**Molds.** `rt-cmd-pattern` OK (`#[command(name=…)]` + `display_order` 91/92, `Option<String>` args with help doc, dispatch arm `.as_deref()`). `rt-report-pattern` OK (`FindingCollectReport`: `ok` first + documented, `skip_serializing_if` on the optional, every `Vec` sorted before return, `#[must_use]` builder on `&Path`, thin printing `run`, tempdir tests in-file). `rt-outcome-pattern` OK (`MarkFindingOutcome`: closed enum, no serde, idempotent `AlreadyRouted` variant documented, core returns `Result`, `process::exit` only in `run`).

## Acceptance Criteria — each run, each green

| AC | Command | Result | Control |
|---|---|---|---|
| 1 | `cargo test -p mustard-core finding_item` | 5 passed | `checklist_round_trips_with_done_state` 1 passed |
| 2 | `…finding_collect_reads_both_sources` | ok | `run_close_gates_allows_when_everything_passes` ok |
| 3 | `…finding_collect_preserves_declared_route` | ok | idem ok |
| 4 | `…findings_gate_denies_open_finding` -> `…_and_names_the_command` | ok | `run_close_gates_denies_missing_qa_when_strict` ok |
| 5 | `…findings_gate_allows_when_every_finding_routed` -> `…_including_dropped` | ok | idem ok |
| 6 | `…mark_finding_records_route_and_refuses_without_reason` | ok | idem ok |
| 7 | `cargo test --workspace` | **exit 0, 4757 passed** | — |

## Feature enabled, end-to-end, against the built binary

In a scratch project: `run finding-collect --spec demo` seeded 2 open findings (1 review file, 1 `removal:"survived"`, the honest red correctly excluded); `run mark-finding … --to dropped` with no/blank reason exits 2 with the refusal; with a reason -> `routed`; restated -> `already-routed`; a *different* destination -> exit 1 refusing the silent overwrite, and the sidecar kept the first decision. Then `run emit-phase --to CLOSE` **denied** naming `[proof_ledger] AC-1` plus the exact `mark-finding` line; after routing it, CLOSE passed; `MUSTARD_FINDINGS_GATE_MODE=warn` fell through. Serde is adjacent as decided: `"routed":{"kind":"dropped","reason":"…"}`.

## MAJOR — the gate is not reached on the documented close path

The sub-gate is correctly placed in `run_close_gates` (debt -> checklist -> **findings** -> QA -> build), exactly where the approved plan pointed. But `run_close_gates` is reached only from `emit-phase --to CLOSE` and the legacy PreToolUse hook. The documented everyday close chain (`plugin/commands/pr.md:91`) is `close-orchestrate`, which runs its own gate list and calls `complete_spec::finalize` directly; `finalize` writes the `pipeline.phase: CLOSE` event through its own emitter at `apps/rt/src/commands/spec/complete_spec.rs:557`, never through `emit_phase::run_at` — so `gate_close_for_spec` does not run there. Success metric 1 ("nenhuma spec fecha com achado sem destino") is therefore unmet on the `/pr` path. Not a deviation from the plan: the sibling debt and checklist gates have the identical reach.

## MAJOR — reviewer-side granularity

`apps/rt/src/commands/review/finding_collect.rs:260` mints **one finding per findings FILE**, with the statement being the first quotable line capped at 240 chars. A `findings.md` carrying six reviewer findings becomes one record, and one `--to dropped --reason "…"` settles all six — five of which never reach the sidecar. AC-2 speaks of "os dois" (the two sources), so the AC as written is met, but the spec's promise that "por que ninguém agiu nisso?" gets an auditable answer is only partly realized on the reviewer side.

## MINOR — AC-1's "byte-idêntico" is proved narrowly

`write_meta` still injects `phase/scope/lang/checkpoint: null` into a two-key sidecar (observed live). What is actually byte-stable is the absence of the `findings` key plus the collector's no-write path (`finding_collect_without_sources_writes_no_key`). Pre-existing `Meta` behaviour, not introduced here.

## MINOR — arg shape diverges from the sibling

`mark-finding` declares `--id/--to/--reason` as `Option<String>` and validates by hand, whereas the neighbouring `mark-checklist-item` uses a required `String` for `--reason`. Deliberate (it buys the didactic refusal text), but it diverges from the sibling.

## NOTE — the gate writes to disk

The findings sub-gate runs the collector in-process and persists `meta.json`. Documented, atomic, error-swallowed, and unreachable from a hook except on a pipeline-state CLOSE write, so no Guards violation — but a gate that writes is worth knowing about.
