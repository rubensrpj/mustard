# Wave 2 — Hooks emitem números reais via core::economy::writer

## PRD

Os hooks do `apps/rt` hoje emitem economia simulada (`tokens_saved: 0`, JSON inline construído à mão) e custo de contexto ad-hoc. Esta wave reescreve os 5 pontos de emissão para chamar exclusivamente `core::economy::writer::*`, garantindo que toda telemetria de economia, custo de contexto e custo de API passe pela API tipada entregue em [[wave-1-core-economy]].

## Contexto

A wave 1 entregou o módulo `packages/core/src/economy/` com `writer`, `estimator`, `SavingsSource`, `ContextCostFrame`, `SavingsRecord`, `ApiCostFrame` e `SpanRecord`. Sem esta wave 2 os hooks continuam gravando zeros/JSON manual, e os dashboards do moat ficam vazios apesar do schema novo estar pronto. As waves 3 (ingestão), 4 (atribuição) e 7 (página Economia) dependem destes números reais.

## Usuários/Stakeholders

- Operadores que abrem o dashboard de economia/moat e esperam ver números reais.
- Agentes do pipeline cujas decisões (downgrade de modelo, bloqueio de bash, corte de output) precisam virar `SavingsRecord`.
- Equipe de instrumentação que valida custo de contexto por wave/spec.

## Métrica de sucesso

- 0 ocorrências de `tokens_saved: 0` literal nos 5 arquivos do escopo.
- 0 construções de JSON inline para eventos de economia/custo nesses arquivos — toda emissão vai via `writer::record_*`.
- `cargo test -p mustard-rt` permanece verde.

## Não-Objetivos

- Não tocar em nenhum outro hook fora dos 5 listados.
- Não alterar a API de `core::economy` (consumo apenas).
- Não construir UI/dashboard (wave 6/7).
- Não alterar o schema do DB (W1 já fechou as tabelas necessárias).
- Não implementar atribuição cruzada agent↔span (W4 faz; W2 só popula campos que existem).

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Build do rt passa — Command: `cargo check -p mustard-rt`
- [x] AC-2: Testes do rt passam — Command: `cargo test -p mustard-rt`
- [x] AC-3: `bash_guard` deixa de cravar 0 — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/bash_guard.rs','utf8');if(/tokens_saved:\s*0/.test(t))throw new Error('still hardcoded 0')"`
- [x] AC-4: `model_routing` deixa de cravar 0 — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/model_routing.rs','utf8');if(/tokens_saved:\s*0/.test(t))throw new Error('still hardcoded 0')"`
- [x] AC-5: Todos os 4 hooks importam `core::economy::writer` — Command: `node -e "['bash_guard','model_routing','budget','tracker'].forEach(h=>{const t=require('fs').readFileSync('apps/rt/src/hooks/'+h+'.rs','utf8');if(!t.includes('economy::writer'))throw new Error(h+' missing import')})"`
- [x] AC-6: `spec_extract` emite `ContextCostFrame` — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/spec_extract.rs','utf8');if(!t.includes('ContextCostFrame'))throw new Error('ContextCostFrame not used')"`
- [x] AC-7: `tracker` finaliza span via writer — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/tracker.rs','utf8');if(!t.includes('record_api_cost') && !t.includes('record_span'))throw new Error('tracker not calling writer span/api_cost')"`
- [x] AC-8: Hooks não constroem JSON manual de savings — Command: `node -e "const fs=require('fs');['apps/rt/src/hooks/bash_guard.rs','apps/rt/src/hooks/model_routing.rs','apps/rt/src/hooks/budget.rs'].forEach(f=>{const t=fs.readFileSync(f,'utf8');if(/\"source\"\s*:\s*\"(RtkRewrite|ModelRoutingDowngrade|BashGuardBlock|BudgetOutputCut)\"/.test(t))throw new Error(f+' still building inline savings JSON')})"`

## Plano

Cada um dos 5 arquivos passa a importar `core::economy::writer` e `core::economy::estimator` e substituir a emissão atual por chamada tipada. `bash_guard.rs` e `model_routing.rs` deixam de construir `tokens_saved: 0` e passam a chamar `writer::record_savings` com `SavingsSource::BashGuardBlock` e `SavingsSource::ModelRoutingDowngrade`, respectivamente, derivando `tokens_saved` via `estimator::estimate_input_tokens` sobre o comando bloqueado ou o prompt redirecionado. `budget.rs` chama `writer::record_savings` com `SavingsSource::BudgetOutputCut` sempre que o output é truncado. `spec_extract.rs` troca a emissão ad-hoc de subtração de wave-slice por `writer::record_context_cost(ContextCostFrame { … })`, populando `wave_slice_bytes`, `slice_bytes`, `recipe_bytes`, `prefix_stable_bytes` a partir dos bytes já calculados na função. `tracker.rs` agrupa a finalização do span em duas chamadas: `writer::record_span` (frame da API) e `writer::record_api_cost` (custo derivado via `estimator::model_pricing_usd_micros_per_million`).

## Informações da Entidade

Esta wave consome a API do módulo `economy` entregue em [[wave-1-core-economy]]; sem entidade nova.

## Arquivos (5)

```
apps/rt/src/hooks/bash_guard.rs       (modify — substituir inline JSON + tokens_saved:0 por record_savings(BashGuardBlock))
apps/rt/src/hooks/model_routing.rs    (modify — substituir tokens_saved:0 por record_savings(ModelRoutingDowngrade) + delta de pricing)
apps/rt/src/hooks/budget.rs           (modify — emitir record_savings(BudgetOutputCut) no ramo de truncamento)
apps/rt/src/hooks/tracker.rs          (modify — finalizar span via record_span + record_api_cost lendo bytes do payload)
apps/rt/src/run/spec_extract.rs       (modify — emitir record_context_cost(ContextCostFrame{wave_slice_bytes,...}))
```

## Tarefas

### Backend Hook Agent

- [ ] **`apps/rt/src/hooks/bash_guard.rs:~1417`** — remover construção inline `serde_json::json!({ "source": "bash_guard", "tokens_saved": 0, … })` e chamar `core::economy::writer::record_savings(&conn, SavingsRecord { source: SavingsSource::BashGuardBlock, tokens_saved: estimator::estimate_input_tokens(&blocked_cmd, &model) as i64, model_target: Some(model.clone()), project_path, spec_id: ctx.spec_id.clone(), wave_id: ctx.wave_id.clone(), agent_id: Some(ctx.agent_id.clone()), ts: now_epoch_ms() })?`. `blocked_cmd` vem do payload `PreToolUse.tool_input.command`; `model` vem de `ctx.model` ou env `CLAUDE_MODEL`. `project_path` de env `CLAUDE_PROJECT_PATH`.
- [ ] **`apps/rt/src/hooks/model_routing.rs:~417`** — remover `tokens_saved: 0` e JSON ad-hoc; após decidir downgrade `from_model -> to_model`, calcular `let tokens = estimator::estimate_input_tokens(&prompt, &from_model) as i64;` e chamar `writer::record_savings(&conn, SavingsRecord { source: SavingsSource::ModelRoutingDowngrade, tokens_saved: tokens, model_target: Some(to_model.clone()), project_path, spec_id, wave_id, agent_id: Some(ctx.agent_id.clone()), ts: now_epoch_ms() })?`. `prompt` é o texto que iria ao modelo (já lido na função).
- [ ] **`apps/rt/src/hooks/budget.rs`** — no ramo onde output é truncado para caber no budget, calcular `let saved = estimator::estimate_output_tokens(&dropped_tail, &model) as i64;` e chamar `writer::record_savings(&conn, SavingsRecord { source: SavingsSource::BudgetOutputCut, tokens_saved: saved, model_target: Some(model.clone()), project_path, spec_id, wave_id, agent_id: Some(ctx.agent_id.clone()), ts: now_epoch_ms() })?`. `dropped_tail` é o slice removido (`original[budget_limit..]`). Remover qualquer emissão legada do mesmo ponto.
- [ ] **`apps/rt/src/hooks/tracker.rs`** — na finalização do span (atualmente `PostToolUse`/`Stop`), substituir emissão atual por duas chamadas: (a) `writer::record_span(&conn, SpanRecord { … })` com `prompt_size_bytes = payload.tool_input.to_string().len() as i64`, `return_size_bytes = payload.tool_response.to_string().len() as i64`; (b) `writer::record_api_cost(&conn, ApiCostFrame { … })` derivando `let (in_micros_per_m, out_micros_per_m) = estimator::model_pricing_usd_micros_per_million(&model);` então `cost_usd_micros = (in_micros_per_m * input_tokens + out_micros_per_m * output_tokens) / 1_000_000`. `input_tokens`/`output_tokens` saem do payload da API (`usage.input_tokens`, `usage.output_tokens`); na ausência, estimar via `estimator::estimate_input_tokens` / `estimate_output_tokens` sobre os bytes do payload. Manter session_id, ts, request_id já capturados.
- [ ] **`apps/rt/src/run/spec_extract.rs`** — no bloco onde hoje subtrai `wave_slice` do prompt e emite evento ad-hoc, construir `ContextCostFrame { prompt_size_bytes, prefix_stable_bytes, slice_bytes, recipe_bytes, wave_slice_bytes, return_size_bytes: 0, retry_overhead_bytes: 0, agent_id: ctx.agent_id.clone(), wave_id: ctx.wave_id.clone(), spec_id: ctx.spec_id.clone(), project_path, ts: now_epoch_ms() }` e chamar `writer::record_context_cost(&conn, frame)?`. Os bytes vêm das variáveis já calculadas na função (não recomputar). Remover o `serde_json::json!` legado e seu `emit_event`.
- [ ] Rodar `cargo check -p mustard-rt` e `cargo test -p mustard-rt` — ambos devem passar.

## Dependências

- [[wave-1-core-economy]] — fornece `writer::*`, `estimator::*`, `SavingsSource`, `ContextCostFrame`, `SavingsRecord`, `ApiCostFrame`, `SpanRecord`.

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Depende de: [[wave-1-core-economy]]
- Desbloqueia: [[wave-4-attribution]]
- Recebe memória: [[wave-1-core-economy]] (signatures dos writers, lookup-table de pricing, enum SavingsSource)
- Grava memória: `{hooks_migrated: [...], estimator_calls: [...], context_cost_emissions: [...]}` para [[wave-4-attribution]]

## Limites

Em escopo: `apps/rt/src/hooks/{bash_guard,model_routing,budget,tracker}.rs`, `apps/rt/src/run/spec_extract.rs`.

Fora de escopo: qualquer arquivo em `apps/dashboard/**`, `packages/core/**` (já entregue em W1), outros hooks (`session_start`, `close_gate`, etc.).

## Concerns

- **`bash_guard` usa `SavingsSource::BashGuardBlock` no site de `rtk-rewrite`** — spec dizia literalmente `BashGuardBlock` (honrado), mas o enum também tem `RtkRewrite`, que seria semanticamente mais correto para esse site específico (a 1417 é rewrite, não block). REVIEW deve decidir se troca `BashGuardBlock` → `RtkRewrite` no `bash_guard.rs:1438-1474` ou se mantém para casar com a intenção do PRD.
- **`wave_id` lido de `MUSTARD_ACTIVE_WAVE`** — env var não setada hoje pelo orquestrador. Records de W2 saem com `wave_id: None` até W4 (Atribuição) wirear a injeção da env. Aceitável: `wave_id` é optional no schema, e o agregado por wave volta a funcionar quando W4 entrega.
- **Connection plumbing via helper privado por hook** — em vez de estender API pública de `mustard-core` no meio do wave, cada um dos 5 arquivos abre `SqliteEventStore::for_project(project_dir)` localmente. Trade-off: 5 cópias do helper agora vs 1 entry point central depois. REVIEW pode propor consolidar em `mustard_core::economy::store::open_for(project_path)` na W4.
