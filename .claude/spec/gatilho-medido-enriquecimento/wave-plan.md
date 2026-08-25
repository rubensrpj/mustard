---
id: wave.gatilho-medido-enriquecimento.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.gatilho-medido-enriquecimento.1-backend]] | backend | — | o portao de base passa a medir a lacuna do enriquecimento e a reportar em stderr, e o comentario deixa de citar a porta selada |
| 2 | [[wave.gatilho-medido-enriquecimento.2-docs]] | docs | [[wave.gatilho-medido-enriquecimento.1-backend]] | a prosa semeada do roteador ganha a regra que le o sinal, e um teste trava prosa e codigo no mesmo literal |

## Acceptance Criteria
- AC-1 — when o repositorio tem molde candidato que nenhum agente autorou, then a medida conta esse molde em vez de devolver vazio. Command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::counts_molds_with_no_author -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
- AC-2 — when um subprojeto tem `## Guards` ainda no esqueleto pendente, then a medida nomeia esse subprojeto. Command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::names_a_subproject_whose_guards_are_still_a_scaffold -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
- AC-3 — when nao existe censo no projeto, then a lacuna volta vazia e o portao fica em silencio. Command: `cargo test -p mustard-rt --lib commands::event::enrichment_gap::tests::no_census_means_an_empty_gap -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
- AC-4 — when a prosa semeada do roteador e comparada com o codigo do portao, then as duas metades carregam o MESMO literal de sinal. Command: `cargo test -p mustard-rt --test plugin_prose_matches_shipped_behaviour the_router_prose_names_the_signal_the_gate_emits -- --exact 2>&1 | grep -q "test result: ok. 1 passed"`
