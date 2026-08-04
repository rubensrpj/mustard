---
id: wave.every-finding-must-have-declared.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.every-finding-must-have-declared.1-core]] | core | — | O achado ganha modelo: um item que ou tem destino declarado, ou esta aberto — espelhando ChecklistItem. |
| 2 | [[wave.every-finding-must-have-declared.2-collect]] | collect | [[wave.every-finding-must-have-declared.1-core]] | O coletor deterministico le as duas fontes que hoje ninguem le e semeia os achados no sidecar. |
| 3 | [[wave.every-finding-must-have-declared.3-gate]] | gate | [[wave.every-finding-must-have-declared.2-collect]] | A porta que declara o destino e o portao que recusa o fecho enquanto sobrar achado aberto. |

## Acceptance Criteria
- AC-1 — quando um meta.json carrega achados, entao um achado sem destino e distinguido de um descartado com motivo, e um sidecar escrito antes do campo existir volta byte-identico. Command: `cargo test -p mustard-core finding_item` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-core checklist_round_trips_with_done_state`
- AC-2 — quando o coletor roda numa spec que tem findings do revisor E um ledger com criterio `survived`, entao os dois aparecem como achados abertos no meta.json, cada um com o motivo que sua fonte escreveu. Command: `cargo test -p mustard-rt finding_collect_reads_both_sources` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_allows_when_everything_passes`
- AC-3 — quando o coletor roda de novo depois de um destino declarado, entao o destino sobrevive e o achado nao volta a ficar aberto. Command: `cargo test -p mustard-rt finding_collect_preserves_declared_route` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_allows_when_everything_passes`
- AC-4 — quando sobra achado aberto, entao o fecho e recusado e a recusa nomeia o achado e o comando que o resolve. Command: `cargo test -p mustard-rt findings_gate_denies_open_finding` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_denies_missing_qa_when_strict`
- AC-5 — quando todo achado tem destino declarado, inclusive um descartado COM motivo, entao o portao de achados deixa o fecho seguir. Command: `cargo test -p mustard-rt findings_gate_allows_when_every_finding_routed` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_denies_missing_qa_when_strict`
- AC-6 — quando o operador declara o destino pela porta, entao o sidecar registra destino E motivo, e a porta recusa a declaracao sem motivo. Command: `cargo test -p mustard-rt mark_finding_records_route_and_refuses_without_reason` Expect: `[1-9][0-9]* passed` Control: `cargo test -p mustard-rt run_close_gates_denies_missing_qa_when_strict`
- AC-7 — o workspace continua verde. Command: `cargo test --workspace` Expect: `[1-9][0-9]* passed`
