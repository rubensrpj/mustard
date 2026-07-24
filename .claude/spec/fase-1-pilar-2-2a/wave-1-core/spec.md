---
id: wave.fase-1-pilar-2-2a.1-core
---

# wave-1-core

## Summary

Janela de tempo no EconomyScope + filtro por ts nos readers de economia

## Network

- Parent: [[spec.fase-1-pilar-2-2a]]

## Tasks

- [ ] Adicionar o mecanismo de janela (from/to ISO) ao EconomyScope, compondo com o escopo atual (Projeto/Spec/Wave/Comparar) — nunca substituindo o escopo
- [ ] Filtrar os eventos NDJSON por ts dentro da janela nos readers de economia (economy_summary, per_agent_costs, per_spec_costs, per_wave_costs, savings_breakdown, context_routing_quality, metric_token_summary, per_phase_token_summary)
- [ ] Fail-open: sem janela, ou ts ausente/inparseavel, agrega todos os eventos como hoje (sem regressao de escopo)
- [ ] Testes: uma janela [from,to] exclui os eventos fora e mantem os de dentro; janela ausente agrega tudo

## Files

- `packages/core/src/domain/economy/scope.rs`
- `packages/core/src/domain/economy/reader.rs`
