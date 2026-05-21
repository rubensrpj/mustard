# Wave 2 — Hooks emitem números reais via core::economy::writer

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: queued
### Phase: PLAN
### Scope: full (wave)
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt

## PRD

Hoje `bash_guard.rs:1417` e `model_routing.rs:417` emitem eventos com `tokens_saved: 0` cravado. `budget.rs` mede `input_chars` para decidir bloquear mas nunca persiste. `spec_extract.rs` calcula `slice_bytes` só no stdout. `tracker.rs` tem comentário explícito "wave-slice bytes depend on a [missing] feature". Esta wave substitui todos os write-paths por chamadas ao `core::economy::writer::*` da W1, usando `core::economy::estimator` para estimativa real quando precisa, e populando os campos novos (`prompt_size_bytes`, `slice_bytes`, `recipe_bytes`, `wave_slice_bytes`, `return_size_bytes`) que ninguém grava hoje. Resultado: dashboards param de mostrar zero.

## Acceptance Criteria

- [ ] AC-1: Build do rt passa — Command: `cargo check -p mustard-rt`
- [ ] AC-2: Testes do rt passam — Command: `cargo test -p mustard-rt`
- [ ] AC-3: `bash_guard` deixa de cravar 0 — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/bash_guard.rs','utf8');if(/tokens_saved:\\s*0/.test(t))throw new Error('still hardcoded 0')"`
- [ ] AC-4: `model_routing` deixa de cravar 0 — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/model_routing.rs','utf8');if(/tokens_saved:\\s*0/.test(t))throw new Error('still hardcoded 0')"`
- [ ] AC-5: Todos os 5 hooks importam `core::economy::writer` — Command: `node -e "['bash_guard','model_routing','budget','tracker'].forEach(h=>{const t=require('fs').readFileSync('apps/rt/src/hooks/'+h+'.rs','utf8');if(!t.includes('economy::writer'))throw new Error(h+' missing import')})"`

## Plano

`bash_guard`, `model_routing`, `budget`, `tracker`, `spec_extract` chamam `writer::record_savings()`/`record_context_cost()` em vez de montar JSON inline. Estimativa de tokens vem de `core::economy::estimator`. Cada record carrega `project_path` + `spec_id?` + `wave_id?` + `agent_id?`.

## Dependências

- [[wave-1-core-economy]]: API do writer + estimator + tipos.

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Depende de: [[wave-1-core-economy]]
- Desbloqueia: [[wave-4-attribution]]
- Recebe memória: [[wave-1-core-economy]] (signatures dos writers, lookup-table de pricing, enum SavingsSource)
- Grava memória: `{hooks_migrated: [...], estimator_calls: [...], context_cost_emissions: [...]}` para [[wave-4-attribution]]

## Limites

Em escopo: `apps/rt/src/hooks/{bash_guard,model_routing,budget,tracker}.rs`, `apps/rt/src/run/spec_extract.rs`.

Fora de escopo: qualquer arquivo em `apps/dashboard/**`, `packages/core/**` (já entregue em W1), outros hooks (`session_start`, `close_gate`, etc.).
