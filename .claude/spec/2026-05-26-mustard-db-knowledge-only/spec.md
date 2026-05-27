# mustard.db apenas knowledge + memory

### Stage: Close
### Outcome: Cancelled
### Flags:
### Scope: full
### Checkpoint: 2026-05-26T00:00:00Z
### Lang: pt-BR
### Parent: [[2026-05-26-no-sqlite-git-source-of-truth]]

> **CANCELLED 2026-05-26** — Absorvida por `[[2026-05-26-no-sqlite-git-source-of-truth]]`. Esta spec propunha shrink do `mustard.db` mantendo 5 tabelas (knowledge_patterns + memory_*); a spec sucessora **elimina o DB por completo** e move knowledge/memory para markdown atomic versionado em git. Reescrever os emitters duas vezes (uma para shrink, outra para delete) duplicaria trabalho — daí a absorção. Sem migração de dados (dev phase, sem usuários em prod).

## PRD

## Contexto

O `mustard.db` na raiz do repo está em 5.5 MB. Medição via `mustard-rt run db-maintain` mostra que 5.1 MB (93%) vêm da tabela `events` + `events_fts_data` + 6 índices que foram **conceitualmente removidos** pela spec W5 da unification (2026-05-24-mustard-unification), substituídos por NDJSON per-spec em `.claude/spec/{name}/.events/*.ndjson` + `.blobs/`. O schema atual em `packages/core/src/store/sqlite_schema.sql` não declara mais essas tabelas — elas persistem só como resíduo nos bancos no disco. Além disso, o banco ainda carrega `pipeline_events`, `sessions`, `pipeline_amend_window`, `specs`, `savings_records`, `context_cost_frames` — todas elas também são eventos/cache derivado de alto volume que ferem o princípio "banco leve, NDJSON pra eventos". A premissa final do usuário é que `mustard.db` deve conter **apenas conhecimento de longo prazo**: knowledge_patterns, memory_decisions, memory_lessons, agent_memory, memory_feedback (mais os espelhos FTS5 e triggers). Tudo mais migra para NDJSON per-spec, arquivo local de sessão, ou cache rebuildable que não precisa estar no banco.

## Usuários/Stakeholders

Maintainer único (Rubens). Indireto: qualquer projeto-alvo onde `mustard init` foi rodado e o `.claude/.harness/mustard.db` ficou inchando ao longo do tempo. Memória [[project_db_bloat_per_spec_events]] confirma a intenção original.

## Métrica de sucesso

Após `mustard init` fresh em projeto novo, `.claude/.harness/mustard.db` tem ≤100 KB com apenas as 5 tabelas knowledge+memory + FTS5 espelhos + triggers. `mustard-rt run db-maintain` retorna `per_table[]` listando apenas as 5 tabelas declaradas. Nenhum dashboard query, nenhum hook, nenhum subcomando do `mustard-rt` quebra (verificado por `cargo test` workspace verde).

## Não-Objetivos

- Migrar dados históricos dos bancos existentes (dev-phase, [[feedback_no_migration_dev_phase]] — apaga e recria).
- Recriar `pipeline_events`, `sessions`, etc. em outro banco SQLite separado (vão para NDJSON per-spec ou estado local).
- Mudar o desenho de `.events/*.ndjson` + `.blobs/` que já está estabelecido.
- Tocar `telemetry.db` (separado, escopo próprio — `run_usage`, `usage_totals`).
- Adicionar novos índices ou otimizações nas 5 tabelas que ficam.

## Critérios de Aceitação

- [ ] AC-1: `packages/core/src/store/sqlite_schema.sql` declara APENAS knowledge_patterns + memory_decisions + memory_lessons + agent_memory + memory_feedback + os 3 FTS5 espelhos + triggers — Command: `bash -c 'count=$(grep -E "^CREATE TABLE IF NOT EXISTS" packages/core/src/store/sqlite_schema.sql | grep -vE "knowledge_patterns|memory_decisions|memory_lessons|agent_memory|memory_feedback" | wc -l); test "$count" = "0"'`
- [ ] AC-2: `mustard-rt run db-maintain` retorna `per_table[]` listando apenas as 5 tabelas + FTS5 (zero menção a events/pipeline_events/sessions/etc) — Command: `bash -c 'cargo run -q -p mustard-rt -- run db-maintain | node -e "let s=\"\";process.stdin.on(\"data\",c=>s+=c).on(\"end\",()=>{const r=JSON.parse(s);const bad=r.per_table.filter(t=>!t.table.match(/^(knowledge_patterns|memory_decisions|memory_lessons|agent_memory|memory_feedback|.+_fts.*|sqlite_.*)\\b/));process.exit(bad.length===0?0:1)})"'`
- [ ] AC-3: `mustard.db` em projeto fresh ≤ 100 KB — Command: `bash -c 'cd $(mktemp -d) && cargo run -q -p mustard-cli --manifest-path /caminho/Cargo.toml -- init --yes && sz=$(stat -c%s .claude/.harness/mustard.db); test "$sz" -le 102400'`
- [ ] AC-4: `cargo build` workspace passa — Command: `cargo build`
- [ ] AC-5: `cargo test` workspace passa (exceto pre-existing failures documentadas) — Command: `cargo test --workspace`
- [ ] AC-6: Dashboard builda sem erros — Command: `pnpm --filter mustard-dashboard build`

## Plano

## Informações da Entidade

Não há entidade de domínio nova. Os agregados tocados são: schema do `mustard.db` (`packages/core/src/store/sqlite_schema.sql`), todos os run subcommands que escrevem em tabelas que saem (~15+ arquivos em `apps/rt/src/run/`), readers do dashboard que consomem essas tabelas via Tauri (`apps/dashboard/src-tauri/`), hooks que persistem savings/cost (`apps/rt/src/hooks/budget.rs`, `bash_guard.rs`, etc).

## Arquivos

### Schema + DDL
- `packages/core/src/store/sqlite_schema.sql` (MODIFY — remover 6 CREATE TABLE: pipeline_events, sessions, pipeline_amend_window, specs, savings_records, context_cost_frames + seus índices)
- `packages/core/src/store/migrations.rs` (MODIFY — pode virar no-op ou só guardar VACUUM)
- `packages/core/src/store/sqlite_store.rs` (MODIFY — remover writers/readers das tabelas que saem; criar writers NDJSON per-spec onde for substituto)

### Eventos lifecycle (era `pipeline_events`) → NDJSON
- `apps/rt/src/run/emit_pipeline.rs` (MODIFY — gravar em NDJSON per-spec em vez de SQLite)
- `apps/rt/src/run/emit_phase.rs` (MODIFY — idem)
- `apps/rt/src/run/event_writer_ndjson.rs` (MODIFY — provavelmente já é o writer; expandir kinds aceitos)
- `apps/rt/src/run/event_route.rs` (MODIFY — routing entre NDJSON kinds)
- `apps/rt/src/run/event_projections.rs` (MODIFY — leitura NDJSON em vez de SQL queries)
- `apps/rt/src/run/active_specs.rs` (MODIFY — listar specs por NDJSON scan)
- `apps/rt/src/run/pipeline_state_ingest.rs` (MODIFY — ingest baseado em NDJSON)
- `apps/rt/src/run/pipeline_summary.rs` (MODIFY)
- `apps/rt/src/run/complete_spec.rs` (MODIFY)

### Cache (era `specs`) → rebuilt from NDJSON
- `apps/rt/src/run/rebuild_specs.rs` (MODIFY — já reconstrói; agora vira fonte primária em vez de cache)
- Qualquer reader que consultava `specs` table → migra para usar `rebuild_specs` output

### Sessions (era `sessions`) → arquivo local
- `apps/rt/src/run/transcript_watcher.rs` (MODIFY — armazena session metadata em `.claude/.harness/sessions.json` ou similar)
- `apps/rt/src/hooks/session_start.rs` (MODIFY)

### Amend window (era `pipeline_amend_window`) → estado local da spec
- `apps/rt/src/hooks/amend_capture.rs` (MODIFY)
- `apps/rt/src/run/amend_finalize.rs` (MODIFY)
- Pode persistir em `.claude/spec/{name}/.amend-window.json`

### Economy (era `savings_records` + `context_cost_frames`) → NDJSON
- `packages/core/src/economy/store.rs` (MODIFY — writer NDJSON em vez de SQLite)
- `packages/core/src/economy/writer.rs` (MODIFY)
- `apps/rt/src/run/economy_capture_baseline.rs` (MODIFY)
- `apps/rt/src/run/economy_reconcile.rs` (MODIFY)
- `apps/rt/src/run/economy_report.rs` (MODIFY)
- Todos hooks que gravam savings: `bash_guard.rs`, `model_routing.rs`, `tracker.rs`, etc — escrever via NDJSON

### Readers/Dashboard
- `apps/dashboard/src-tauri/src/lib.rs` (MODIFY — Tauri commands que consultavam pipeline_events/sessions/etc)
- `apps/dashboard/src/lib/dashboard.ts` (MODIFY — reader-side TS)

### Cleanup físico
- Deletar 5 mustard.db (`.claude/.harness/`, `apps/cli/.claude/.harness/`, `apps/dashboard/.claude/.harness/`, `apps/rt/.claude/.harness/`, `packages/core/.claude/.harness/`) — recriados na próxima abertura

## Tarefas

### Wave 1 — Schema redesign + reader-side
- [ ] Atualizar `sqlite_schema.sql` removendo 6 CREATE TABLE + comentários
- [ ] Atualizar `migrations.rs` removendo migrações dessas tabelas
- [ ] Atualizar `sqlite_store.rs` removendo métodos write/read das tabelas que saem
- [ ] Build packages/core verde

### Wave 2 — Lifecycle events → NDJSON
- [ ] Expandir `event_writer_ndjson.rs` para aceitar kinds que iam pra `pipeline_events`
- [ ] Modificar `emit_pipeline.rs` / `emit_phase.rs` para gravar NDJSON em vez de SQLite
- [ ] Modificar readers (`event_projections.rs`, `active_specs.rs`, `pipeline_summary.rs`, `complete_spec.rs`) para ler do NDJSON

### Wave 3 — Cache/sessions/amend
- [ ] `specs` table → consumir output de `rebuild_specs.rs` em vez de query SQL
- [ ] `sessions` → arquivo local
- [ ] `pipeline_amend_window` → estado local da spec

### Wave 4 — Economy → NDJSON
- [ ] `savings_records` e `context_cost_frames` → escrever em NDJSON per-spec ou per-session
- [ ] Readers do dashboard atualizados para consumir NDJSON

### Wave 5 — Dashboard + cleanup
- [ ] Tauri commands atualizados
- [ ] TypeScript readers atualizados
- [ ] Deletar 5 mustard.db + testar reabertura fresh

### Wave 6 — Validação
- [ ] cargo test workspace
- [ ] dashboard build
- [ ] mustard init em tmpdir → AC-3
- [ ] db-maintain final → AC-2

## Dependências

- [[2026-05-26-template-agnostic-audit]] já entregou refator i18n (W5/W7) e recipe death (W6). Não conflita; ambas independentes.
- Memórias relevantes: [[project_db_bloat_per_spec_events]], [[feedback_no_attach_sqlite]], [[feedback_no_migration_dev_phase]], [[feedback_everything_measurable]].

## Limites

- MODIFY: schema + ~20-25 arquivos Rust + ~3-5 arquivos dashboard
- DELETE: 5 mustard.db físicos
- FORA: telemetry.db (escopo próprio); `.events/*.ndjson` + `.blobs/` (já existe, expandir); knowledge/memory tables (ficam); FTS5 espelhos das 5 que ficam; specs históricas em `.claude/spec/*` (não migrar headers nem rows)
- BREAKING: rows existentes em pipeline_events/sessions/etc são perdidos (aceitar — dev phase)
