# W7 — Shared memory hardening (cross-session, scope-by-cluster)
### Stage: Plan
### Outcome: Active
### Flags: 

## Contexto

Memória entre agentes (`mustard.db`) existe mas é primitiva: scope-by-spec apenas, sem feedback bidirecional, sem decay automático, skills geradas pelo scan não influenciam matching. W1 entregou frontmatter padronizado (`appliesTo`/`tags`/`scope`/`entities`) — esta wave aproveita para scope-by-cluster.

## Tarefas

- [x] **T7.1** — Tabela `agent_memory` (já DDL em W0.T0.5): adicionar lógica de write/read em `apps/rt/src/run/memory.rs`. Campos: id, session_id, spec, wave, role, summary, details, confidence, status, at, last_used. FTS5 mirror.
- [x] **T7.2** — Tabela `memory_feedback`: deprecate/bump/supersede/use (já DDL em W0). Lógica em `memory.rs`.
- [x] **T7.3** — Subcomandos novos:
  - `mustard-rt run memory search --query X --spec Y --cluster Z` — full-text + filtro por scope
  - `mustard-rt run memory feedback --id N --kind {deprecate|bump|supersede|use}`
  - `mustard-rt run memory write --verify` — agente grava memória com verificação pós-write
- [x] **T7.4** — `memory-ingest --agent-memory` migra `.claude/.agent-memory/_index.json` (legacy JSON) para SQLite. Após migração, remove o diretório.
- [x] **T7.5** — Lazy decay on read: ao ler memória, aplicar fator `confidence * (1 - days_since_last_used / 30)`. Memórias com confidence < 0.3 não retornam por default.
- [x] **T7.6** — Filtro padrão na injeção: `spec=current OR (spec IS NULL AND confidence>=0.8)`. Estende para `cluster` quando wave tem `appliesTo` declarado: `OR (cluster IN (waveAppliesTo) AND confidence>=0.5)`. *(Helper exposto em `default_injection_select`; consumidor em `session_start` fica para W11.)*
- [x] **T7.7** — `mustard-rt run memory cross-wave --spec X --wave N` (já existe parcial em mega-spec) ganha filtro por `cluster` da wave atual.

## Critérios de Aceitação

- [x] **AC-W7.1** — `agent_memory` aceita writes via `mustard-rt run memory write`. Command: smoke test.
- [x] **AC-W7.2** — `mustard-rt run memory search --help` existe.
- [x] **AC-W7.3** — Diretório `.claude/.agent-memory/` removido após migração. Command: `rtk node -e "if(require('fs').existsSync('.claude/.agent-memory'))process.exit(1)"` *(diretório já ausente no repo; subcomando `memory-ingest --agent-memory` testado: roda sem erros, retorna `deleted=true`.)*
- [x] **AC-W7.4** — `cross-wave` aceita `--cluster`. Command: `rtk mustard-rt run memory cross-wave --help`
- [x] **AC-W7.5** — `cargo test -p mustard-rt memory` passa.

## Limites

`apps/rt/src/run/memory.rs`, `apps/rt/src/run/memory_ingest.rs`, `apps/rt/src/run/mod.rs`.

OUT: schema do `mustard.db` (W0.T0.5 fechou).

## Role

rt
