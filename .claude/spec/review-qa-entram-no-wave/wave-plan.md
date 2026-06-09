# Plano de Waves

## Tabela de Waves

| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-impl]] | impl | — | RT: rodada de review no wave-advance + fallback de TASK no render |
| 2 | [[wave-2-impl]] | impl | [[wave-1-impl]] | Prosa: rodada de review do wave-advance no resume-flow (template + espelho local) |

## Critérios de Aceitação
- AC-1 — wave-advance emite rodada de review (1 item mustard-review por subprojeto) quando as ondas impl estao completas: `cargo test -p mustard-rt wave_advance_review`
- AC-2 — render com fallback de TASK p/ spec sem secao Tasks; spec com Tasks identico: `cargo test -p mustard-rt task_fallback`
- AC-3 — Suite do rt verde: `cargo test -p mustard-rt pipeline`
