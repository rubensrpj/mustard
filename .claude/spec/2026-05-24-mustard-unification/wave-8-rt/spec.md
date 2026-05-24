# W8 — Shared memory hardening

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: full
### Checkpoint: 2026-05-24T19:30:00Z
### Lang: pt-BR
### Parent: 2026-05-24-mustard-unification

## Contexto

Memória entre agentes hoje:

1. `memory_decisions` e `memory_lessons` em `mustard.db` não têm coluna `spec` — multi-spec contention untested.
2. `agent_memory` mora em `.claude/.agent-memory/_index.json` (rolling 20) — perde após 20 entries, sem FTS, não queryable.
3. Agente escreve fire-and-forget sem retry/validação.
4. Sem feedback: agente não pode marcar entrada como "desatualizada".
5. Cross-wave memory existe (`memory cross-wave`) mas NÃO é auto-injetado.

W5.T5.6 já criou o DDL final no schema redesenhado. Esta onda implementa a lógica.

## Tarefas

- [ ] **T8.1.** Tabela `agent_memory` (já criada no DDL em W5.T5.6) — implementar writer/reader em `apps/rt/src/run/memory.rs`. Schema: `(id, session_id, spec, wave, role, actor_kind, actor_id, summary, details, confidence, status, at, last_used)` + FTS5 `agent_memory_fts`.
- [ ] **T8.2.** Migração one-shot `mustard-rt run memory-ingest --agent-memory` lê `.claude/.agent-memory/_index.json` e popula `agent_memory`. Renomeia dir antigo para `.agent-memory.archived/` (não deleta).
- [ ] **T8.3.** Tabela `memory_feedback` (já criada em W5.T5.6) — implementar `mustard-rt run memory feedback --target-table X --target-id N --action {depreciate|bump|supersede|use} [--reason "..."] [--superseded-by N]`. Insere row em `memory_feedback` + UPDATE atomic na target table.
- [ ] **T8.4.** Subcomando `mustard-rt run memory search --term X [--spec Y] [--top N]`. FTS5 sobre `agent_memory_fts`, `memory_decisions_fts`, `memory_lessons_fts`, `knowledge_patterns_fts`. Saída JSON com hits unificados ranqueados.
- [ ] **T8.5.** Wrapper `mustard-rt run memory write --type {decision|lesson|agent|pattern} --json X --verify`. Insere, lê de volta via `last_insert_rowid()`, retorna stdout `{ok:true,id:N}` ou `{ok:false,reason:"..."}`. Exit code sempre 0 (fail-open).
- [ ] **T8.6.** Decay lazy on read: `effective_confidence = confidence * max(0.2, 1 - days_since_last_used/180)`. Reader ordena por `effective_confidence`. `last_used` atualizado quando consumido por SessionStart inject ou `agent-prompt-render`.
- [ ] **T8.7.** Filtro scope-by-spec: `(spec = current_spec OR (spec IS NULL AND confidence >= 0.8 AND status='active'))`. Override `MUSTARD_MEMORY_CROSS_SPEC=1` para `/scan` e auditorias.
- [ ] **T8.8.** Confidence inicial por tipo: `0.8` agent_memory, `0.7` decision/lesson, `0.3` pattern.
- [ ] **T8.9.** Hooks novos em `apps/rt/src/hooks/`:
  - `auto_capture_summary` (PostToolUse(Task) observer): parse `<MEMORY>...</MEMORY>` ou `Resumo:` no `tool_response`, insere em `agent_memory` automaticamente.
  - `memory_write_verify` (PostToolUse(Bash) observer): quando command matches `^mustard-rt run memory write`, valida stdout JSON e emite `memory.written` event no SQLite.
- [ ] **T8.10.** Emit `pipeline.economy.operation.invoked { operation: "memory-search|memory-write|memory-feedback", duration_ms }` para `/economia`.

## Files

- `apps/rt/src/run/memory.rs` (estender: search/feedback/write--verify)
- `apps/rt/src/run/memory_ingest.rs` (estender: flag `--agent-memory`)
- `apps/rt/src/run/mod.rs` (registrar novos subcomandos `search` e `feedback`)
- `apps/rt/src/hooks/auto_capture_summary.rs` (novo)
- `apps/rt/src/hooks/memory_write_verify.rs` (novo)
- `apps/rt/src/registry.rs` (registrar novos hooks)
- `packages/core/src/store/sqlite_store.rs` (queries com decay + scope filter)

## Critérios de Aceitação

- [ ] **AC-8.1.** Tabela `agent_memory` tem colunas `spec`, `wave`, `confidence`, `status`. Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema agent_memory\"',{encoding:'utf8'});for(const k of ['spec','wave','confidence','status']){if(!out.includes(k))process.exit(1)}"`
- [ ] **AC-8.2.** `mustard-rt run memory search --term X` retorna FTS hits. Command: `rtk mustard-rt run memory search --term 'spec' --top 3 --json | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!Array.isArray(j.hits))process.exit(1)})"`
- [ ] **AC-8.3.** `mustard-rt run memory feedback --target-table agent_memory --target-id 1 --action depreciate` muda `status` da linha. Command: setup fixture + verify.
- [ ] **AC-8.4.** `mustard-rt run memory write --type decision --json '{"content":"x"}' --verify` retorna stdout `{ok:true,id:N}`. Command: `rtk mustard-rt run memory write --type decision --json '{"content":"test"}' --verify | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const j=JSON.parse(s);if(!j.ok)process.exit(1)})"`
- [ ] **AC-8.5.** `.claude/.agent-memory/_index.json` migrado para `agent_memory` table (após `memory-ingest --agent-memory`). Dir antigo renomeado para `.agent-memory.archived/`.
- [ ] **AC-8.6.** Hook `auto_capture_summary` registrado. Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/registry.rs','utf8');if(!/auto_capture_summary/.test(t))process.exit(1)"`
- [ ] **AC-8.7.** Reader respeita scope-by-spec por padrão. Command: teste `memory_reader_filters_by_spec`.
- [ ] **AC-8.8.** `rtk cargo test -p mustard-rt memory` passa.

## Notas

- Paralelizável com W7.
- DDL já foi feito em W5.T5.6 (schema redesenhado do zero).
- O hook `auto_capture_summary` é cético: só extrai se o markdown está na forma canônica esperada.
