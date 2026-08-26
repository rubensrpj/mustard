---
id: wave.teto-retentativa.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.teto-retentativa.1-vocabulary]] | vocabulary | — | o nome proprio do evento de retentativa e o emissor que o escreve no log do spec, sem encostar na telemetria de friccao |
| 2 | [[wave.teto-retentativa.2-pipeline]] | pipeline | [[wave.teto-retentativa.1-vocabulary]] | o teto lido em advance(): conta as retentativas do log, tira da rodada o wave que estourou e poe no lugar um item de escalacao |
| 3 | [[wave.teto-retentativa.3-plugin]] | plugin | — | o teto de turnos dentro de cada subagente do plugin, declarado no frontmatter |

## Acceptance Criteria
- AC-2 — Quando o classificador de rota recebe o nome do evento novo, entao ele o coloca no balde `pipeline` e o balde `friction` continua reconhecendo `retry.attempt`. Command: `grep -q 'fn retry_event_is_not_routed_as_friction' apps/rt/src/shared/events/route.rs && cargo test -p mustard-rt retry_event_is_not_routed_as_friction 2>&1` Expect: `test result: ok\. 1 passed`
- AC-1 — Quando o log `.events/` de um spec carrega retentativas de um wave em numero igual ou maior que o teto, entao `wave-advance` devolve a rodada sem aquele wave e com um item de escalacao que o nomeia. Command: `grep -q 'fn retry_ceiling_pulls_wave_from_round' apps/rt/src/commands/pipeline/wave_advance.rs && cargo test -p mustard-rt retry_ceiling_pulls_wave_from_round 2>&1` Expect: `test result: ok\. 1 passed`
- AC-3 — Quando os tres agentes do plugin sao lidos, entao cada frontmatter declara `maxTurns` com um inteiro positivo. Command: `grep -lE '^maxTurns: [1-9][0-9]*$' plugin/agents/mustard-guards.md plugin/agents/mustard-patterns.md plugin/agents/mustard-review.md` Expect: `mustard-review\.md`
