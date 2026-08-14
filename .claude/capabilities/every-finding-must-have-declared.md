---
id: cap.every-finding-must-have-declared
status: active
---

# every finding must have declared

### Requirement: The system SHALL satisfy the acceptance criteria of spec every-finding-must-have-declared.

#### Scenario: AC-1
- when: 
- then: quando um meta.json carrega achados, entao um achado sem destino e distinguido de um descartado com motivo, e um sidecar escrito antes do campo existir volta byte-identico.
- command: `cargo test -p mustard-core finding_item`

#### Scenario: AC-2
- when: 
- then: quando o coletor roda numa spec que tem findings do revisor E um ledger com criterio `survived`, entao os dois aparecem como achados abertos no meta.json, cada um com o motivo que sua fonte escreveu.
- command: `cargo test -p mustard-rt finding_collect_reads_both_sources`

#### Scenario: AC-3
- when: 
- then: quando o coletor roda de novo depois de um destino declarado, entao o destino sobrevive e o achado nao volta a ficar aberto.
- command: `cargo test -p mustard-rt finding_collect_preserves_declared_route`

#### Scenario: AC-4
- when: 
- then: quando sobra achado aberto, entao o fecho e recusado e a recusa nomeia o achado e o comando que o resolve.
- command: `cargo test -p mustard-rt findings_gate_denies_open_finding`

#### Scenario: AC-5
- when: 
- then: quando todo achado tem destino declarado, inclusive um descartado COM motivo, entao o portao de achados deixa o fecho seguir.
- command: `cargo test -p mustard-rt findings_gate_allows_when_every_finding_routed`

#### Scenario: AC-6
- when: 
- then: quando o operador declara o destino pela porta, entao o sidecar registra destino E motivo, e a porta recusa a declaracao sem motivo.
- command: `cargo test -p mustard-rt mark_finding_records_route_and_refuses_without_reason`

#### Scenario: AC-8
- when: 
- then: quando o fecho vem pelo caminho do dia a dia (close-orchestrate) e existe achado sem destino, entao o fecho e recusado ali tambem, e nao apenas no emit-phase --to CLOSE
- command: `cargo test -p mustard-rt close_orchestrate_blocks_on_open_finding`

#### Scenario: AC-7
- when: 
- then: o workspace continua verde.
- command: `cargo test --workspace`

## Covers

## Specs
- [[spec.every-finding-must-have-declared]]

## Related

