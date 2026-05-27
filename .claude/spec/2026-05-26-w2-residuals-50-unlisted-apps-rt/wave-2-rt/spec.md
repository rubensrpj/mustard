# W2 — run/ emit + memory + amend sweep (16 violations / 8 files)
### Stage: Close
### Outcome: Completed
### Flags: 

## Contexto

Sweep de pipeline writers em `apps/rt/src/run/`. Esses arquivos escrevem em `.claude/spec/{name}/.events/`, `.claude/spec/{name}/spec.md`, `.harness/`. Substituir `.join(".claude")` por `ClaudePaths::spec(slug)`, `ClaudePaths::harness()`, `ClaudePaths::spec_events(slug)` conforme o método específico.

`emit_pipeline.rs` é o maior alvo (7 violações) e a fonte canônica de spec mutations — extra cuidado para não introduzir regressão nos AC commands existentes (já há cobertura via `pipeline_state_projection_test.rs`).

## Arquivos (lista enumerada)

| # | Arquivo | Violações |
|---|---------|-----------|
| 1 | `apps/rt/src/run/emit_pipeline.rs` | 7 (linhas 263, 280, 467, 506, 567, 622, 1008) |
| 2 | `apps/rt/src/run/emit_phase.rs` | 1 (linha 124) |
| 3 | `apps/rt/src/run/event_writer_ndjson.rs` | 2 (linhas 79, 86) |
| 4 | `apps/rt/src/run/memory_cross_wave.rs` | 2 (linhas 105, 266) |
| 5 | `apps/rt/src/run/memory_ingest.rs` | 1 (linha 291) |
| 6 | `apps/rt/src/run/spec_memory.rs` | 1 |
| 7 | `apps/rt/src/run/amend_finalize.rs` | 1 (linha 152) |
| 8 | `apps/rt/src/run/resume_bootstrap.rs` | 1 (linha 113) |

## Tarefas

- [ ] **TF2.1** — Ler `packages/core/src/claude_paths.rs` para mapear métodos relevantes (`spec(slug)`, `spec_events(slug)`, `spec_md(slug)`).
- [ ] **TF2.2** — Para cada arquivo em ordem (começar por `emit_pipeline.rs` que tem mais violações), substituir todos os callsites.
- [ ] **TF2.3** — Rodar `rtk cargo test -p mustard-rt --quiet --tests pipeline_state_projection` após `emit_pipeline.rs` para detectar regressão precoce.
- [ ] **TF2.4** — `rtk cargo check -p mustard-rt` ao final da wave.

## Critérios de Aceitação

- [ ] **AC-W2.1** — Zero `.join(".claude")` em `apps/rt/src/run/{emit_pipeline,emit_phase,event_writer_ndjson,memory_cross_wave,memory_ingest,spec_memory,amend_finalize,resume_bootstrap}.rs` fora de tests gated. Command: `rtk node "C:/Users/ruben/.claude/jobs/3922ef93/ac_tf1.js" 2>&1 | rtk grep "run/\(emit\|event_writer\|memory\|amend\|resume_boot\|spec_memory\)"` deve ser vazio.
- [ ] **AC-W2.2** — `rtk cargo check -p mustard-rt` passa.
- [ ] **AC-W2.3** — `rtk cargo test -p mustard-rt --tests pipeline_state_projection_test --quiet` passa.

## Limites

IN: 8 arquivos listados em `apps/rt/src/run/`.
OUT: outros arquivos de `apps/rt/src/run/`, `apps/rt/src/hooks/`, `apps/rt/tests/`.

## Role

rt-impl
