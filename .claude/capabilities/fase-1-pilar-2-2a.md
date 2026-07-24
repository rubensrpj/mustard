---
id: cap.fase-1-pilar-2-2a
status: active
---

# fase 1 pilar 2 2a

### Requirement: The system SHALL satisfy the acceptance criteria of spec fase-1-pilar-2-2a.

#### Scenario: AC-1
- when: um reader de economia recebe uma janela `[from, to]`
- then: eventos NDJSON com `ts` fora da janela são excluídos do agregado (e os de dentro permanecem).
- command: `cargo test -p mustard-core -- economy_time_window`

#### Scenario: AC-2
- when: nenhuma janela é dada (ou um evento não tem `ts` parseável)
- then: o reader agrega todos os eventos como hoje — fail-open, sem regressão de escopo.
- command: `cargo test -p mustard-core -- economy_time_window_absent`

#### Scenario: AC-3
- when: um comando `dashboard_economy_*` recebe uma janela no `EconomyScopeDto`
- then: ele a repassa ao reader do core e o resultado reflete só o período.
- command: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml -- economy_window`

#### Scenario: AC-4
- when: a página Economia é renderizada
- then: o seletor de janela expõe exatamente as quatro opções (1d/7d/15d/30d) e trocar a opção re-consulta a economia com o novo recorte (o `from` derivado compõe com o escopo).
- command: `pnpm --dir apps/dashboard build`

#### Scenario: AC-5
- when: 
- then: o build e os testes do workspace passam verdes.
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.fase-1-pilar-2-2a]]

## Related

