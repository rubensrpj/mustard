# Review — packages/core (+ rt cascade)

## AC results — every named test run, real output

- AC-1 `cargo test -p mustard-core finding_item` → `5 passed, 624 filtered out` PASS (control `checklist_round_trips_with_done_state` → `1 passed`)
- AC-2 `cargo test -p mustard-rt finding_collect_reads_both_sources` → `2 passed` PASS
- AC-3 `finding_collect_preserves_declared_route` → `1 passed` (lib + bin) PASS
- AC-4 `findings_gate_denies_open_finding` → `1 passed` PASS (name is `…_and_names_the_command`, prefix matches the filter)
- AC-5 `findings_gate_allows_when_every_finding_routed` → `1 passed` PASS
- AC-6 `mark_finding_records_route_and_refuses_without_reason` → `1 passed` PASS
- AC-7 `cargo test --workspace` → `4757 passed, 0 failed, 6 ignored (70 suites)` PASS. `cargo clippy --workspace --all-targets` exit 0 (no new deny).

## Guards / molds

Clean. No `*-pattern` skill matches `domain/spec/contract.rs` (all molds scope to `domain/model/view/**` or `domain/economy/**`). Core writes go through `write_meta` → `io::fs::write_atomic`; `domain/spec` stays IO-free; no `unwrap/expect` outside `#[cfg(test)]`. The rt four-registration guard is satisfied for both new commands (enum + dispatch + `run_command_surface` 93 + `template_parity` whitelist); `mustard-rt run --help` lists `finding-collect` and `mark-finding`.

## Live proof the feature works

Built binary, temp project: `emit-phase --to CLOSE` exits **1** naming both producers and the exact `mark-finding` line; after routing both, exit **0**. Default is strict without setting `MUSTARD_FINDINGS_GATE_MODE`.

## CRITICAL — the gate is silently defeated on the normal retry path

`finding_collect.rs:209/232`: identity is `(source, file-stem | criterion-id)`, never the discovery, while the statement is refreshed from disk. `review_result` overwrites `review/findings.md` under the same name on every review round, so round 2's finding is born already routed by round 1's decision. Proven end-to-end:

```
# round 1 finding routed  --to dropped --reason "cosmetic, not worth a wave"
# findings.md overwritten with "ROUND TWO: the collector inherits a stale decision"
$ mustard-rt run emit-phase --to CLOSE --spec demo2   → exit=0
  "statement": "ROUND TWO: the collector inherits a stale decision",
  "routed": { "kind": "dropped", "reason": "cosmetic, not worth a wave" }
```

CLOSE passed with a finding nobody ever decided about — exactly the "denunciar deixa de significar alguma coisa" the unit exists to close. Same hole on the ledger side: `AC-1` flipping `survived` → `evidence-removed` is a *different* discovery under the same id and inherits the route.

## MAJOR — the collector destroys the one datum it says it preserves

`finding_collect.rs:233` builds only from `fresh`, so a source that transiently stops reporting drops the record *and its declared destination*. Proven: ledger `survived` → mark `--to criterion` → ledger `red` (`"stale": 1`) → ledger `survived` again → `"open": 1` and `routed` gone. This contradicts the module doc ("a re-collection preserves it VERBATIM") and the unit's own DECISIONS entry ("a silent rewrite loses the one datum no collector can reproduce") — here it is a silent delete.

## MINOR — the gate ignores the collector's own `ok`

`close_gates.rs:572` ignores `FindingCollectReport::ok`: a spec dir with findings but no readable `meta.json` blocks CLOSE on findings that `mark-finding` refuses to route ("take a collection with `finding-collect` first", which itself errors `meta-not-found`) — circular, escapable only via `MUSTARD_FINDINGS_GATE_MODE=warn`. Low reachability (scaffolded specs always carry `meta.json`).

## MINOR — one finding per reviewer file

One finding per reviewer *file* with the first quotable line as the statement: a findings file holding five findings is settled by one destination that names only the first.

## Change requests

The two CHANGE REQUESTS ("ar", "segue") carry no requirement, so nothing was silently dropped there.
