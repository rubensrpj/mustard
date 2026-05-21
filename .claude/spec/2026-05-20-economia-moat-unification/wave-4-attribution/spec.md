# Wave 4 — Atribuição por agente (join session_id ↔ tool_use_id)

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: completed
### Phase: EXECUTE
### Scope: full (wave)
### Checkpoint: 2026-05-21T05:05:00Z
### Lang: pt

## PRD

Depois das W2+W3, o `mustard.db` tem custo real Anthropic (via OTEL/JSONL com `session_id`) e tem `agent.start`/`agent.stop` events (com `agent_id`, `wave`, `spec`, timestamp). Esta wave entrega o **join** que ninguém mais faz: pivota tudo por `(spec, wave, agent)` para responder "esse Task custou X USD, recebeu Y tokens de contexto, retornou Z tokens". Atribuição é o moat — Claude Code, RTK, claude-devtools não atribuem por agente. Reader expõe `per_agent_costs`/`per_spec_costs`/`per_wave_costs` com agregação correta nos 4 scopes (Project/Spec/Wave/AllProjects). JSONL traz `tool_use_id` por mensagem assistant; correlaciona com `agent.start`/`agent.stop` por janela temporal + `session_id`.

## Acceptance Criteria

- [x] AC-1: Testes passam — Command: `cargo test -p mustard-core`
- [x] AC-2: Reader `per_agent_costs` faz join real — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/reader.rs','utf8');if(!/JOIN|join/.test(t)&&!/per_agent_costs.*\\n.*spans.*agent_id/s.test(t))throw new Error('per_agent_costs lacks join logic')"`
- [x] AC-3: Teste de atribuição existe — Command: `node -e "if(!require('fs').existsSync('packages/core/tests/economy_attribution.rs'))throw new Error('attribution test missing')"`
- [x] AC-4: Reader retorna sem erro em scope vazio (AllProjects sem dados) — Command: `cargo test -p mustard-core --test economy_attribution test_empty_all_projects`
- [x] AC-5: `memory cross-wave` retorna markdown não-vazio para wave>=2 quando há memória prévia — Command: `bash -c 'out=$(rtk mustard-rt run memory cross-wave --spec 2026-05-20-economia-moat-unification --wave 4); [ -n "$out" ] && echo "$out" | grep -q "wave-"'` (absorvido da spec superseded `2026-05-20-metrics-writers-pipeline-key`)
- [x] AC-6: Reader tem teste de regressão pro caso parent-spec/child-wave attribution — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/reader.rs','utf8');if(!t.includes('parent_spec_child_wave_attribution'))throw new Error('regression test name missing')"` (absorvido da spec superseded `2026-05-20-metrics-writers-pipeline-key`)

## Plano

Estende `reader.rs` da W1 com lógica de join. JSONL parser da W3 já popula `spans` com `session_id` + `request_id` + `tool_use_id` (este novo campo precisa entrar no schema da spans table — migration adicional aqui). `agent.start` events do `tracker` já carregam `agent_id` + `spec` + `wave` + `session_id`. Join: `spans` LEFT JOIN `events WHERE event = 'agent.start' AND payload.tool_use_id = spans.tool_use_id` para mapear cada API call ao agente que a originou. Fallback: se `tool_use_id` não bate, usa janela temporal (`spans.ts` entre `agent.start.ts` e `agent.stop.ts` + mesmo `session_id`). Reader pivota e devolve `Vec<AgentCost>` ordenado.

## Informações da Entidade

Estende `mustard_core::economy::reader` (entregue em W1) com lógica real de join. Sem entidade nova; adiciona coluna `tool_use_id` na tabela `spans` (já estendida em W1/W3) via migration v4 append-only.

## Arquivos (5)

```
packages/core/src/economy/reader.rs           (modify — substituir aproximação por SQL join real em per_agent/per_spec/per_wave)
packages/core/src/store/migrations.rs         (modify — APPEND migration v4: ALTER spans ADD COLUMN tool_use_id TEXT)
packages/core/src/economy/sources/transcript.rs (modify — popular tool_use_id quando message.content[].type=tool_use)
packages/core/src/economy/sources/otel.rs     (modify — popular tool_use_id quando atributo gen_ai.tool_use_id presente)
packages/core/tests/economy_attribution.rs    (new — 6 testes: tool_use_id match, temporal window fallback, parent-spec/child-wave, empty AllProjects, per_spec aggregation, per_wave aggregation)
```

## Tarefas

### Core Attribution Agent

- [ ] Adicionar migration v4 em `packages/core/src/store/migrations.rs` (APPEND-only): `ALTER TABLE spans ADD COLUMN tool_use_id TEXT;` + índice `CREATE INDEX IF NOT EXISTS idx_spans_tool_use_id ON spans(tool_use_id);`. Re-runnable via probe (segue padrão de migrations existentes).
- [ ] Atualizar `economy::sources::transcript::ingest` e `economy::sources::otel::ingest` (em packages/core) para popular `SpanRecord.tool_use_id` quando disponível no payload externo. (Transcript: `message.content[].id` quando `type=tool_use`. OTEL: atributo `gen_ai.tool_use_id` se exposto pelo collector.) Manter `None` quando ausente — fallback temporal cobre.
- [ ] Reescrever `economy::reader::per_agent_costs(conn, scope)` substituindo a aproximação proporcional do W1 por SQL real: primário `JOIN events_table ON events.event='agent.start' AND JSON_EXTRACT(events.payload,'$.tool_use_id') = spans.tool_use_id`, fallback temporal `events.session_id = spans.session_id AND spans.ts BETWEEN events.ts AND COALESCE(stop.ts, events.ts + 3600000)`. GROUP BY `agent_id`, SUM `cost_usd_micros` + `input_tokens + output_tokens`.
- [ ] Atualizar `per_spec_costs` e `per_wave_costs` análogo: agrupa por `spec_id`/`wave_id` extraídos de `agent.start.payload`. Filtra por scope. AllProjects faz fan-out via `MultiProjectReader`.
- [ ] Remover comentário "W2 debt" em `reader.rs:147-152` (aproximação proporcional) — substituído por join real.
- [ ] Garantir que `per_agent_costs` retorne `Vec` vazio (não erro) em scope sem dados — útil para AllProjects vazio e novos projetos.
- [ ] Criar `packages/core/tests/economy_attribution.rs` com 6 testes integrados em `tempdir`:
  - `test_tool_use_id_join_primary` — grava span+agent.start com mesmo `tool_use_id`, valida atribuição correta
  - `test_temporal_window_fallback` — grava span sem `tool_use_id` mas dentro da janela do agente, valida fallback
  - `test_empty_all_projects` — `EconomyScope::AllProjects(vec![])` retorna Vec vazio sem erro (AC-4)
  - `test_per_spec_aggregation` — 3 spans em 2 specs diferentes, valida soma por spec
  - `test_per_wave_aggregation` — spans em waves diferentes do mesmo spec, valida agregação por wave
  - `test_parent_spec_child_wave_attribution` (literal — AC-6 grep) — evento gravado com `spec=<parent>` é atribuído à child wave correta via `wave_id` no payload
- [ ] Verificar AC-5 ainda funciona: `mustard-rt run memory cross-wave --spec X --wave N` retorna markdown não-vazio quando há memória de waves anteriores. Se quebrou pela mudança de reader, ajustar.
- [ ] Rodar `cargo check -p mustard-core` + `cargo test -p mustard-core` + AC-2..AC-6 individuais — todos verdes.

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

## Concerns

- **`tool_use_id` via `extra` map em vez de campo tipado** — adicionar `tool_use_id: Option<String>` em `SpanRecord` quebraria o struct-init em `apps/rt/src/hooks/tracker.rs:112` (fora do boundary W4). Workaround: o canal `extra["tool_use_id"]` permite que adapters W3 propaguem o id sem tocar `apps/rt`, e o writer extrai do `extra` na hora de persistir. REVIEW pode propor tipar (migrar W2 junto numa pass de cleanup) na W4.5 ou tactical-fix.
- **Filtro de scope movido para fora da CTE** — W1 filtrava `spans.spec = ?1` antes da agregação. W4 não pode: `spans.spec` não é mais autoritativo (vem de `agent.start`). Custo: a CTE atribui TODOS os spans antes do filtro de scope. Para escala atual (≤10k spans por projeto) é ok; vira problema se escalar para 1M+.
- **Terceiro fallback usa coluna do span próprio** quando não há `agent.start` correspondente (legacy spans pré-W4 ou fixtures). Preserva W1 backward compat sem tocar API pública, mas significa que atribuição "errada" é silenciosa — não há sinal claro de "este span ficou sem origem". REVIEW pode propor um log/métrica de "spans sem atribuição".
