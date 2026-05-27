# Summary foundation — validação de exports e self-test subcommand

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-27T09:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec de [[2026-05-26-no-sqlite-git-source-of-truth]] — wave 1A. **Summary foundation (validação + self-test subcommand).** Trabalho de `e37e5a1` já entregou `packages/core/src/summary/{mod,writer,schema_md}.rs`. Esta sub-spec: (1) verifica `lib.rs` exporta `SpecSummaryDoc` + writer; (2) teste unitário serializa `SpecSummaryDoc` confirmando campo `version` numérico; (3) `mustard-rt run pipeline-summary --self-test` (subcommand novo, ~30 linhas) chama writer e devolve JSON.

## Critérios de Aceitação

- [x] AC-1A-1: `cargo test -p mustard-core summary` passa + `mustard-rt run pipeline-summary --self-test` produz JSON com campo `version` numérico. Command: `cargo test -p mustard-core summary && cargo run -q -p mustard-rt -- run pipeline-summary --self-test`

## Plano

## Arquivos

- `packages/core/src/lib.rs`
- `packages/core/src/summary/mod.rs` (ajuste)
- `apps/rt/src/run/pipeline_summary.rs` (novo)
- `apps/rt/src/run/mod.rs` (registrar)

## Tarefas

1. `packages/core/src/lib.rs` — verificar e adicionar `pub use summary::{SpecSummaryDoc, write_summary}` (ou equivalente) se ausente
2. `packages/core/src/summary/mod.rs` — ajustar exports públicos: `SpecSummaryDoc` e writer devem ser `pub` e acessíveis via `mustard_core::summary`; adicionar teste unitário que constrói `SpecSummaryDoc` mínimo e serializa com `serde_json::to_string`, assertando `j["version"].is_number()`
3. `apps/rt/src/run/pipeline_summary.rs` — CREATE: subcommand `--self-test` que instancia `SpecSummaryDoc` com valores mínimos, chama writer, imprime JSON no stdout e sai com código 0; ~30 linhas; sem argumento extra além de `--self-test`
4. `apps/rt/src/run/mod.rs` — registrar variante `PipelineSummary` no enum `RunCmd` + arm no match dispatch chamando `pipeline_summary::run(args)`

## Dependências

(nenhuma — W1A não depende de outras sub-specs)

## Limites

- CAP RÍGIDO: ≤5 arquivos (já satisfeito por construção)
- Sem stubs preservando nomes SQLite
- Após commit: `git grep -lE "SqliteEventStore|sqlite_store|memory_sqlite" -- 'packages/**/*.rs' 'apps/**/*.rs'` count DEVE decrescer (ou ficar igual se sub-spec não toca esses arquivos — caso W1A que CRIA primitivos novos)
- Benchmarks de performance no AC são binários — passa ou falha
