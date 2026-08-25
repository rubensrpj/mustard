---
id: cap.teto-retentativa
status: active
---

# teto retentativa

### Requirement: The system SHALL satisfy the acceptance criteria of spec teto-retentativa.

#### Scenario: AC-2
- when: 
- then: Quando o classificador de rota recebe o nome do evento novo, entao ele o coloca no balde `pipeline` e o balde `friction` continua reconhecendo `retry.attempt`.
- command: `grep -q 'fn retry_event_is_not_routed_as_friction' apps/rt/src/shared/events/route.rs && cargo test -p mustard-rt retry_event_is_not_routed_as_friction 2>&1`

#### Scenario: AC-1
- when: 
- then: Quando o log `.events/` de um spec carrega retentativas de um wave em numero igual ou maior que o teto, entao `wave-advance` devolve a rodada sem aquele wave e com um item de escalacao que o nomeia.
- command: `grep -q 'fn retry_ceiling_pulls_wave_from_round' apps/rt/src/commands/pipeline/wave_advance.rs && cargo test -p mustard-rt retry_ceiling_pulls_wave_from_round 2>&1`

#### Scenario: AC-3
- when: 
- then: Quando os tres agentes do plugin sao lidos, entao cada frontmatter declara `maxTurns` com um inteiro positivo.
- command: `grep -lE '^maxTurns: [1-9][0-9]*$' plugin/agents/mustard-guards.md plugin/agents/mustard-patterns.md plugin/agents/mustard-review.md`

## Covers

## Specs
- [[spec.teto-retentativa]]

## Related

