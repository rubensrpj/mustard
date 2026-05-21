# Wave 4 — Atribuição por agente (join session_id ↔ tool_use_id)

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: queued
### Phase: PLAN
### Scope: full (wave)
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt

## PRD

Depois das W2+W3, o `mustard.db` tem custo real Anthropic (via OTEL/JSONL com `session_id`) e tem `agent.start`/`agent.stop` events (com `agent_id`, `wave`, `spec`, timestamp). Esta wave entrega o **join** que ninguém mais faz: pivota tudo por `(spec, wave, agent)` para responder "esse Task custou X USD, recebeu Y tokens de contexto, retornou Z tokens". Atribuição é o moat — Claude Code, RTK, claude-devtools não atribuem por agente. Reader expõe `per_agent_costs`/`per_spec_costs`/`per_wave_costs` com agregação correta nos 4 scopes (Project/Spec/Wave/AllProjects). JSONL traz `tool_use_id` por mensagem assistant; correlaciona com `agent.start`/`agent.stop` por janela temporal + `session_id`.

## Acceptance Criteria

- [ ] AC-1: Testes passam — Command: `cargo test -p mustard-core`
- [ ] AC-2: Reader `per_agent_costs` faz join real — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/reader.rs','utf8');if(!/JOIN|join/.test(t)&&!/per_agent_costs.*\\n.*spans.*agent_id/s.test(t))throw new Error('per_agent_costs lacks join logic')"`
- [ ] AC-3: Teste de atribuição existe — Command: `node -e "if(!require('fs').existsSync('packages/core/tests/economy_attribution.rs'))throw new Error('attribution test missing')"`
- [ ] AC-4: Reader retorna sem erro em scope vazio (AllProjects sem dados) — Command: `cargo test -p mustard-core --test economy_attribution test_empty_all_projects`
- [ ] AC-5: `memory cross-wave` retorna markdown não-vazio para wave>=2 quando há memória prévia — Command: `bash -c 'out=$(rtk mustard-rt run memory cross-wave --spec 2026-05-20-economia-moat-unification --wave 4); [ -n "$out" ] && echo "$out" | grep -q "wave-"'` (absorvido da spec superseded `2026-05-20-metrics-writers-pipeline-key`)
- [ ] AC-6: Reader tem teste de regressão pro caso parent-spec/child-wave attribution — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/reader.rs','utf8');if(!t.includes('parent_spec_child_wave_attribution'))throw new Error('regression test name missing')"` (absorvido da spec superseded `2026-05-20-metrics-writers-pipeline-key`)

## Plano

Estende `reader.rs` da W1 com lógica de join. JSONL parser da W3 já popula `spans` com `session_id` + `request_id` + `tool_use_id` (este novo campo precisa entrar no schema da spans table — migration adicional aqui). `agent.start` events do `tracker` já carregam `agent_id` + `spec` + `wave` + `session_id`. Join: `spans` LEFT JOIN `events WHERE event = 'agent.start' AND payload.tool_use_id = spans.tool_use_id` para mapear cada API call ao agente que a originou. Fallback: se `tool_use_id` não bate, usa janela temporal (`spans.ts` entre `agent.start.ts` e `agent.stop.ts` + mesmo `session_id`). Reader pivota e devolve `Vec<AgentCost>` ordenado.

## Dependências

- [[wave-2-hooks-real]]: hooks gravam `agent_id`/`spec_id`/`wave_id` nos payloads
- [[wave-3-ingestion]]: spans table populada com `session_id` e `tool_use_id` reais

## Concerns absorvidas (da spec superseded `2026-05-20-metrics-writers-pipeline-key`)

A spec `metrics-writers-pipeline-key` foi superseded por esta feature (cancelled em 2026-05-20). Dois itens dela não eram cobertos diretamente e foram absorvidos como AC-5 e AC-6 acima:

- **`memory cross-wave` retornando vazio**: a wave-network-standard fechou com o reader em `apps/rt/src/run/memory.rs` retornando markdown vazio quando deveria injetar memória de waves anteriores. AC-5 garante regressão coberta.
- **Parent-spec/child-wave attribution**: eventos gravados com `spec=<parent>` precisam ser atribuídos à child wave correta no reader. AC-6 garante teste de regressão nomeado.

O fix técnico aqui é mais profundo (refactor de domínio) que o patch local que a spec superseded propunha, mas o resultado funcional é equivalente — mais robusto.

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Depende de: [[wave-2-hooks-real]], [[wave-3-ingestion]]
- Desbloqueia: [[wave-6-trace-viewer]], [[wave-7-economia-page]]
- Grava memória: `{join_strategy: "tool_use_id primary, temporal window fallback", per_agent_shape: "...", aggregation_levels: [...]}` para [[wave-6-trace-viewer]] e [[wave-7-economia-page]]

## Limites

Em escopo: `packages/core/src/economy/reader.rs` (extend), `packages/core/src/store/migrations.rs` (APPEND migration para adicionar coluna `tool_use_id` em `spans` se não existir), `packages/core/tests/economy_attribution.rs` (new).

Fora de escopo: hooks (já entregue em W2), adapters (já entregue em W3), UI, qualquer alteração na API pública de writer.
