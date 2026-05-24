# Tactical Fix: Limpar clippy warnings em packages/core

## Contexto

Tactical fix derivado de [[2026-05-22-project-profiler]].

A Wave 1 do project-profiler entregou `apps/rt` com clippy 100% limpo, mas a AC-3 (`cargo clippy -p mustard-rt -- -D warnings`) falha porque o clippy compila transitivamente as dependências do workspace e `packages/core` carrega 114 erros pré-existentes (módulos `economy`, `telemetry`, `reader`). Esses warnings não foram introduzidos pelo project-profiler — vieram acumulando em sessões anteriores que não rodaram clippy global. Limpar agora desbloqueia a AC-3 da Wave 1 e remove o débito antes das próximas waves (W2-W5) também serem auditadas.

## Critérios de Aceitação

- [x] AC-1: `packages/core` passa clippy sem warning — Command: `cargo clippy -p mustard-core -- -D warnings`
- [ ] AC-2: AC-3 original da Wave 1 do project-profiler agora passa — Command: `cargo clippy -p mustard-rt -- -D warnings`
- [x] AC-3: testes do core continuam verdes — Command: `cargo test -p mustard-core`

## Concerns

- AC-2 não foi atingida — premissa estava errada. Os 722 erros restantes em `cargo clippy -p mustard-rt -- -D warnings` são **nativos do crate `apps/rt/`**, não propagados de `packages/core`. Esse débito é coberto pela spec adjacente [[2026-05-23-clippy-pedantic-cleanup-mustard-rt]] (cujo AC-2 — `cargo clippy -p mustard-core` — já está satisfeito pelo trabalho desta sub-spec). Classificação: `CONCERN` (não bloqueante; trabalho redirecionado para a spec correta).

## Arquivos

- `packages/core/src/economy/**` — limpar warnings (sem mudar comportamento)
- `packages/core/src/telemetry/**` — limpar warnings (sem mudar comportamento)
- `packages/core/src/reader/**` — limpar warnings (sem mudar comportamento)
- Demais módulos de `packages/core/src/` conforme surfaçar no `cargo clippy`
