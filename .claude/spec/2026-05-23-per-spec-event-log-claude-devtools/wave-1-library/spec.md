# Wave 1 — RT writer per-spec NDJSON

## PRD

### Contexto

Hoje cada PreToolUse/PostToolUse/SubagentStart/SubagentStop dispara um INSERT na tabela `events` de `mustard.db`. Com ~100-500µs por insert e o WAL serializando todos os escritores, waves paralelas brigam pelo lock e o hot path cresce com o tamanho da pipeline. Esta wave reroteia a escrita: hooks passam a appendar uma linha NDJSON em `.claude/spec/{name}/[wave-N-{role}/]events/{ts-ns}-{run-id}-{pid}.ndjson`. O filename garante disjunção entre escritores concorrentes (cada subagent tem seu arquivo). Payloads `> 4KB` são spillados por SHA-256 para `events/blobs/{ab}/{sha256}.bin` (content-addressed → o mesmo arquivo Read 5x gera 1 blob), e a linha do evento carrega `"$ref": "sha256:..."` no lugar do payload. Eventos de ciclo de vida (`pipeline.status`, `pipeline.phase`, `pipeline.scope`) vão para uma nova mini-tabela `pipeline_events` em `mustard.db` — esses precisam ser query-rápidos pra o dashboard listar "que specs estão ativas". A tabela `events` antiga e `events_fts` são dropadas; sem migração de dados.

### Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-W1-1: `cargo build -p mustard-rt && cargo build -p mustard-core` passam — Command: `cargo build -p mustard-rt && cargo build -p mustard-core`
- [ ] AC-W1-2: `cargo test -p mustard-rt --test ndjson_writer` passa (append + concorrência) — Command: `cargo test -p mustard-rt --test ndjson_writer`
- [ ] AC-W1-3: `cargo test -p mustard-rt --test ndjson_blob_spill` passa (payload 5KB → blob criado) — Command: `cargo test -p mustard-rt --test ndjson_blob_spill`
- [ ] AC-W1-4: `cargo test -p mustard-rt --test parent_id_subagent` passa (Task spawn registra parent_id correto) — Command: `cargo test -p mustard-rt --test parent_id_subagent`
- [ ] AC-W1-5: Tabela `events` foi dropada de `mustard.db` — Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema events\"',{encoding:'utf8'}).trim();process.exit(out===''?0:1)"`
- [ ] AC-W1-6: Tabela `pipeline_events(spec,kind,ts,payload,session_id)` existe — Command: `node -e "const{execSync}=require('child_process');const out=execSync('sqlite3 .claude/.harness/mustard.db \".schema pipeline_events\"',{encoding:'utf8'});process.exit(out.includes('CREATE TABLE')&&out.includes('spec')&&out.includes('kind')?0:1)"`
- [ ] AC-W1-7: `cargo clippy --workspace -- -D warnings` passa — Command: `cargo clippy --workspace -- -D warnings`

## Plano

### Informações da Entidade

Eventos não são entidade de domínio — são infra do harness. O shape canônico vive em `apps/rt/src/events_ndjson/shape.rs`. Não há entry em `entity-registry.json`.

### Arquivos

- `apps/rt/src/events_ndjson/mod.rs` (novo) — façade pública do módulo
- `apps/rt/src/events_ndjson/shape.rs` (novo) — struct `Event` com serde (`ts`, `agent_id`, `parent_id`, `evt_id`, `kind`, `tool`, `label`, `tokens_in`, `tokens_out`, `duration_ms`, `status`, `input`, `output`)
- `apps/rt/src/events_ndjson/path.rs` (novo) — resolução de caminho (spec ativa, wave ativa, ou fallback `.claude/.session/{slug}/`)
- `apps/rt/src/events_ndjson/writer.rs` (novo) — append atômico com `OpenOptions::append`; filename `{ts-ns}-{run-id}-{pid}.ndjson`
- `apps/rt/src/events_ndjson/blob.rs` (novo) — SHA-256 spill com threshold 4KB; write-temp-then-rename para atomicidade
- `apps/rt/src/events_ndjson/parent.rs` (novo) — mapping `subagent_id → parent_evt_id` em memória (cleared em SubagentStop)
- `apps/rt/src/hooks/pre_tool_use.rs` (edição) — emit ndjson
- `apps/rt/src/hooks/post_tool_use.rs` (edição) — emit ndjson + parent_id propagation
- `apps/rt/src/hooks/subagent_start.rs` (edição) — registra parent_id
- `apps/rt/src/hooks/subagent_stop.rs` (edição) — limpa mapping
- `apps/rt/src/run/emit_event.rs` (edição) — roteia kind para SQLite (lifecycle) vs ndjson (tool/agent/output)
- `apps/rt/src/run/emit_pipeline.rs` (edição) — sempre escreve em `pipeline_events`
- `packages/core/src/store/sqlite_schema.sql` (edição) — DROP `events`, `events_fts`, triggers `events_ai`/`events_ad`; CREATE `pipeline_events` + `idx_pipeline_events_spec` + `idx_pipeline_events_ts`
- `packages/core/src/store/sqlite_store.rs` (edição) — métodos `insert_pipeline_event`, `query_pipeline_events_for_spec`
- `apps/rt/tests/ndjson_writer.rs` (novo)
- `apps/rt/tests/ndjson_blob_spill.rs` (novo)
- `apps/rt/tests/parent_id_subagent.rs` (novo)

### Tarefas

#### Library Agent (Wave 1)

- [ ] Criar `apps/rt/src/events_ndjson/{mod,shape,path,writer,blob,parent}.rs`
- [ ] Implementar `append_event(ctx: &EventContext, ev: Event)` com filename `{ts-ns}-{run-id}-{pid}.ndjson`
- [ ] Implementar blob spill em `blob.rs` (SHA-256 + diretório `{ab}/`); write-temp + rename
- [ ] Implementar `parent.rs` (HashMap thread-safe; `register(subagent_id, parent_evt_id)` / `lookup(subagent_id)` / `clear(subagent_id)`)
- [ ] Refatorar 4 hooks (`pre_tool_use`, `post_tool_use`, `subagent_start`, `subagent_stop`) para chamar `append_event`
- [ ] Atualizar `sqlite_schema.sql`: remover events/events_fts/triggers; adicionar `pipeline_events`
- [ ] Refatorar `emit_event.rs` para rotear por kind; `emit_pipeline.rs` sempre vai pra `pipeline_events`
- [ ] Testes `ndjson_writer.rs` (single append, multi-writer paralelo, durabilidade)
- [ ] Testes `ndjson_blob_spill.rs` (payload 5KB; blob criado; ref correto)
- [ ] Testes `parent_id_subagent.rs` (Task spawn → subagent escreve com parent_id setado)
- [ ] `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`

### Dependências

Nenhuma — Wave 1 é o ponto de partida.

### Limites

- **Tocar:** `apps/rt/src/events_ndjson/**`, `apps/rt/src/hooks/{pre_tool_use,post_tool_use,subagent_start,subagent_stop}.rs`, `apps/rt/src/run/{emit_event,emit_pipeline}.rs`, `apps/rt/tests/{ndjson_writer,ndjson_blob_spill,parent_id_subagent}.rs`, `packages/core/src/store/{sqlite_schema.sql,sqlite_store.rs}`.
- **NÃO tocar:** `apps/dashboard/**` (W3), `apps/cli/**` (W5), `packages/core/src/reader/**` (W2), `packages/core/src/projection/**` (W2), `packages/core/src/model/view/**` (W2).
