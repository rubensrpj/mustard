---
id: cap.pergunta-abertura-unidade-pergunta-tipo
status: active
---

# pergunta abertura unidade pergunta tipo

### Requirement: The system SHALL satisfy the acceptance criteria of spec pergunta-abertura-unidade-pergunta-tipo.

#### Scenario: AC-1
- when: o bloco-modelo do roteador é lido
- then: a linha `sai de:` aparece
- command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_asks_the_base_before_the_type`

#### Scenario: AC-2
- when: a regra ao lado do bloco é lida
- then: ela nomeia o teto de opções da
- command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_forbids_pairing_and_pins_hotfix`

#### Scenario: AC-3
- when: a cópia entregue neste projeto é comparada com a semente compilada,
- then: 
- command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour delivered_copy_matches_the_seed_at_the_base_row`

#### Scenario: AC-4
- when: o portão recebe um nome escolhido pelo operador pelo sinal explícito
- then: 
- command: `cargo test -p mustard-rt operator_name_wins_over_the_derivation`

#### Scenario: AC-5
- when: o bloco-modelo é lido
- then: a linha `branch:` se apresenta como campo
- command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour router_offers_the_name_for_correction`

#### Scenario: AC-6
- when: 
- then: quando os injetaveis do roteador sao medidos, entao cada arquivo cabe embutido no contexto sem virar arquivo com previa — a secao ## Dispatch mora num injetavel proprio, pendurado em sessionStart, e o restante do roteador segue em userPromptSubmit
- command: `cargo test -p mustard-cli --test template_budget`

## Covers

## Specs
- [[spec.pergunta-abertura-unidade-pergunta-tipo]]

## Related

