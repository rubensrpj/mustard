---
id: cap.gatilho-medido-enriquecimento
status: active
---

# gatilho medido enriquecimento

### Requirement: The system SHALL satisfy the acceptance criteria of spec gatilho-medido-enriquecimento.

#### Scenario: AC-1
- when: o repositório tem molde candidato que nenhum agente autorou
- then: a medida da
- command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::counts_molds_with_no_author -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-2
- when: um subprojeto tem `## Guards` ainda no esqueleto pendente
- then: a medida nomeia
- command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::names_a_subproject_whose_guards_are_still_a_scaffold -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-3
- when: não existe censo no projeto
- then: a lacuna volta vazia e o portão fica em
- command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::no_census_means_an_empty_gap -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-4
- when: a prosa semeada do roteador é comparada com o código do portão
- then: as duas
- command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour the_router_prose_names_the_signal_the_gate_emits -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`

#### Scenario: AC-5
- when: 
- then: o build do projeto passa verde
- command: `cargo build --workspace`

## Covers

## Specs
- [[spec.gatilho-medido-enriquecimento]]

## Related

