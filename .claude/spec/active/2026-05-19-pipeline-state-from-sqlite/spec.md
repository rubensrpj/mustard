# Migrar runtime state (pipeline + memory + knowledge) para SQLite

### Status: draft
### Phase: PLAN
### Scope: full
### Checkpoint: 2026-05-20T02:30:00Z
### Lang: pt

> **Continua a migração iniciada em `2026-05-19-dashboard-phase-from-sqlite` (CLOSE 2026-05-20).** Aquela spec moveu só o campo `phase` para SQLite. Esta finaliza o trabalho em duas frentes:
> 1. **Pipeline state** — elimina `.claude/.pipeline-states/{spec}.json` como source, derivando o estado completo (status, scope, lang, tasks, currentWave, completedWaves, isWavePlan, lastDispatchFailure, pausedAt, resumeMode) de eventos no `SqliteEventStore`.
> 2. **Memory + knowledge** — migra `.claude/knowledge.json` + `.claude/memory/decisions.json` + `.claude/memory/lessons.json` para tabelas SQLite com índice FTS5 (full-text search built-in, zero deps extras). Mesma motivação: append O(1), busca indexada, sem race em escrita concorrente.
>
> Habilita a próxima fase (sync layer via ElectricSQL ou PowerSync) que pressupõe SQLite como única fonte de verdade para todo runtime state. Vetor (sqlite-vec) fica explicitamente fora — FTS5 cobre o volume atual; vetor entra em spec futura se busca semântica virar necessidade.

## PRD

## Contexto

A spec `2026-05-19-dashboard-phase-from-sqlite` provou que migração reader-by-field para SQLite funciona — `phase` agora deriva de eventos `pipeline.phase`, com projeção `last_phase_for_spec` e gate inline em `emit-phase --to CLOSE`. Restou o resto: `.claude/.pipeline-states/{spec}.json` ainda é source de `status`, `scope`, `lang`, `tasks[]`, `currentWave`, `completedWaves`, `isWavePlan`, `lastDispatchFailure`, `pausedAt`, `pauseReason`, `resumeMode`, `model`. Hoje:

- 6 SKILL.md de pipeline (`feature`, `approve`, `resume`, `close`, `qa`, `bugfix`) escrevem o JSON via Write/Edit em vários pontos.
- O dashboard lê o JSON via `specs_from_fs`, `dashboard_pipelines`, `dashboard_active_pipelines` (`apps/dashboard/src-tauri/src/lib.rs`).
- Hooks de rt (`close_gate`, `path_guard`, `post_edit`, `epic_fold`, `statusline`) leem campos específicos.
- `event_projections.rs` já mistura leitura de JSON + eventos — fragmentação herdada.

A fragmentação tem dois defeitos arquiteturais que essa migração resolve:

1. **Não-atomicidade.** Dois hooks gravando o JSON em paralelo se sobrescrevem (race observada esporadicamente em sessões com paralelismo alto).
2. **Source-of-truth disperso.** Cada campo novo dispara discussão "JSON ou SQL?". Após esta spec, todo runtime state é SQLite-only e a pergunta desaparece.

Os mesmos defeitos atingem `knowledge.json` (padrões confidence-ranked) e `memory/*.json` (decisões e lições): hoje cada append rewrites do arquivo inteiro (custo cresce com tamanho), busca por termo faz `JSON.parse` + filter em memória, e escritas concorrentes de hooks distintos (knowledge update, memory persist, SessionEnd fold) se sobrescrevem. Em SQLite com FTS5: append O(1) constante, busca por termo via índice (`MATCH 'token'`), transação serializa.

Mustard está em dev (memory `no-migration-dev-phase`): a migração corta limpo (ingest one-shot dos JSONs em flight + delete; sem fallback permanente que leia legacy).

## Usuários/Stakeholders

Mantenedores do Mustard. Indiretamente, usuários do `mustard-dashboard` (visibilidade consistente sem stale JSONs e leituras atômicas). Solicitado por Rubens em 2026-05-19 após review da spec-mãe.

## Métrica de sucesso

- `.claude/.pipeline-states/{spec}.json` deixa de ser criado por novos `/feature`.
- `/resume` reconstrói estado completo (status/scope/tasks/currentWave/completedWaves/dispatch_failure/pause/resume_mode) a partir de eventos.
- Dashboard mostra status/tasks/wave sem acessar nenhum path em `.claude/.pipeline-states/`.
- Pipelines em flight migram via ingest one-shot sem perda de estado.
- `.claude/knowledge.json` + `.claude/memory/{decisions,lessons}.json` deixam de ser escritos; hooks emitem direto pras tabelas SQLite.
- `mustard-rt run memory <kind>` continua sendo a CLI (interface preservada), mas internamente faz `INSERT`.
- Busca por termo em decisões/lições/padrões usa `MATCH` (FTS5) em vez de filter em JSON parseado — sub-milissegundo em volumes até 10k entradas.
- `mustard-rt run docs-stale-check` ganha audit que detecta menções de `.pipeline-states/{spec}.json` E de `knowledge.json`/`memory/*.json` como source-of-truth em docs novas.

## Não-Objetivos

- **Não tocar narrativa.** `spec.md`, `wave-plan.md` ficam como source para review humano, git diff e parsing de tasks pelo agente.
- **Não tocar `CLAUDE.md`/`pipeline-config.md` fora da Shared Memory section.** Atualização narrativa cirúrgica apenas.
- **Não migrar `entity-registry.json`.** É cache derivado de `sync-registry` scan; fica como arquivo.
- **Não introduzir busca vetorial (sqlite-vec) agora.** FTS5 (built-in do SQLite, tokenizer `unicode61`) cobre o volume atual de knowledge/memory (dezenas a centenas de entradas) com sub-ms por query. Vetor exigiria: extensão `sqlite-vec` loadable (mudança não-trivial em `libsqlite3-sys` que é shared rt↔dashboard), modelo de embedding (API custa por chamada OU modelo local de ~80MB), e justificativa de volume + uso (busca semântica passa a valer quando "decisão sobre X que falou de Y mas o nome era Z" começa a perder com substring/FTS5). Spec separada quando volume atingir ~1000+ entradas E user reportar miss de busca por palavra-chave.
- **Não migrar `.pipeline-states/*.metrics.json`.** Legacy de spec antiga; cleanup separado.
- **Não introduzir sync layer (ElectricSQL/PowerSync/Litestream).** Explicitamente a próxima fase pós esta migração — sem source-of-truth única, replicação fica inconsistente.
- **Não preservar compatibilidade permanente com pipeline-state JSON.** Ingest one-shot + delete; sem fallback que leia legacy continuamente.
- **Não mudar a wire de `HarnessEvent`.** O struct já é genérico (`event: String`, `payload: Value`, `spec: Option<String>`). Esta spec só convenciona novos valores de `event` e shapes de `payload`.

## Critérios de Aceitação

Critérios binários, executáveis. Cada um roda da raiz do projeto; exit 0 = passou. Padrão `node -e` com `includes()` (cross-shell-safe em Windows cmd.exe — lição da spec-mãe).

- [ ] AC-1: Workspace compila — Command: `cargo build -p mustard-core -p mustard-rt -p mustard-cli`
- [ ] AC-2: Testes rt e dashboard backend passam — Command: `cargo test -p mustard-rt -p mustard-dashboard`
- [ ] AC-3: Dashboard build limpo — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-4: Constantes de eventos novos definidas em mustard-core — Command: `node -e "const c=require('fs').readFileSync('packages/core/src/model/event.rs','utf8');for(const t of ['pipeline.status','pipeline.task.dispatch','pipeline.task.complete','pipeline.wave.complete','pipeline.dispatch_failure','pipeline.pause','pipeline.resume_mode','pipeline.scope']){if(!c.includes(t))process.exit(1)}"`
- [ ] AC-5: Subcomando `emit-pipeline` registrado — Command: `node -e "if(!require('fs').readFileSync('apps/rt/src/run/mod.rs','utf8').includes('EmitPipeline'))process.exit(1)"`
- [ ] AC-6: Projeção `pipeline_state_for_spec` exposta — Command: `node -e "if(!require('fs').readFileSync('apps/rt/src/run/event_projections.rs','utf8').includes('pipeline_state_for_spec'))process.exit(1)"`
- [ ] AC-7: Nenhum dos 6 SKILL.md de pipeline escreve em `.pipeline-states/` — Command: `node -e "const fs=require('fs');for(const n of ['feature','approve','resume','close','qa','bugfix']){const p='apps/cli/templates/commands/mustard/'+n+'/SKILL.md';if(!fs.existsSync(p))continue;const c=fs.readFileSync(p,'utf8');const lines=c.split('\n');for(const l of lines){if(l.includes('.pipeline-states/')&&(l.includes('Write')||l.includes('Edit')||l.includes('write_json'))){console.error(n+': '+l);process.exit(1)}}}"`
- [ ] AC-8: Dashboard `lib.rs` não acessa `.pipeline-states/` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');if(c.includes('.pipeline-states/'))process.exit(1)"`
- [ ] AC-9: Hooks de rt não leem `.pipeline-states/{spec}.json` diretamente — Command: `node -e "const fs=require('fs');for(const f of ['close_gate.rs','path_guard.rs','post_edit.rs']){const c=fs.readFileSync('apps/rt/src/hooks/'+f,'utf8');if(c.includes('.pipeline-states/'))process.exit(1)}"`
- [ ] AC-10: Ingest one-shot existe — Command: `node -e "if(!require('fs').existsSync('apps/rt/src/run/pipeline_state_ingest.rs'))process.exit(1)"`
- [ ] AC-11: `.docs-audit.json` ganha audit dessa migração — Command: `node -e "const m=require('./.claude/.docs-audit.json');if(!m.audits.some(a=>a.from_spec==='2026-05-19-pipeline-state-from-sqlite'))process.exit(1)"`
- [ ] AC-12: `docs-stale-check` da nova audit roda limpo — Command: `cargo run -q -p mustard-rt -- run docs-stale-check --from 2026-05-19-pipeline-state-from-sqlite | node -e "let d='';process.stdin.on('data',c=>d+=c).on('end',()=>{const r=JSON.parse(d);if(r.hits&&r.hits.length>0)process.exit(1)})"`
- [ ] AC-13: Tabelas SQLite de memory/knowledge declaradas — Command: `node -e "const c=require('fs').readFileSync('packages/core/src/io/sqlite_store.rs','utf8');for(const t of ['knowledge_patterns','memory_decisions','memory_lessons','knowledge_fts','memory_decisions_fts','memory_lessons_fts'])if(!c.includes(t))process.exit(1)"`
- [ ] AC-14: `mustard-rt run memory <kind>` insere no SQLite (não no JSON) — Command: `node -e "const c=require('fs').readFileSync('apps/rt/src/run/memory.rs','utf8');if(c.includes('memory/decisions.json')||c.includes('memory/lessons.json')||c.includes('write_json'))process.exit(1)"`
- [ ] AC-15: Ingest de memory/knowledge JSONs existe — Command: `node -e "if(!require('fs').existsSync('apps/rt/src/run/memory_ingest.rs'))process.exit(1)"`
- [ ] AC-16: Busca FTS5 funciona end-to-end — Command: `cargo test -p mustard-core --test sqlite_fts5_smoke 2>&1 | grep -q "test result: ok"`

## Plano

## Informações da Entidade

`HarnessEvent` (em `packages/core/src/model/event.rs`) é genérico via `event: String` + `payload: Value` + `spec: Option<String>`. Esta spec **não muda o struct** — só convenciona novos valores e seus payload shapes:

| event | payload | emitido por |
|---|---|---|
| `pipeline.scope` | `{ scope, lang, model, is_wave_plan, total_waves }` | `/feature` no draft |
| `pipeline.status` | `{ from, to }` | `/approve`, `/resume`, `/close`, `/qa` |
| `pipeline.task.dispatch` | `{ wave, name, agent, role, files, retry_count }` | `/resume` antes de Task |
| `pipeline.task.complete` | `{ wave, name, agent, duration_ms, files_modified, decisions, escalation }` | `/resume` pós-Task |
| `pipeline.wave.complete` | `{ wave, duration_ms }` | `/resume` ao avançar wave |
| `pipeline.dispatch_failure` | `{ agent_type, description, prompt, reason, at }` | `subagent_tracker` hook (substitui `lastDispatchFailure` field) |
| `pipeline.pause` | `{ reason, next_action }` | `/resume` Pause Handoff |
| `pipeline.resume_mode` | `{ mode, escalation }` | `/resume` Step 0.5 |

Projeção `pipeline_state_for_spec(store, spec) -> Option<PipelineStateView>` reconstrói o struct que `.pipeline-states/{spec}.json` carregava — mesma shape, fonte única SQLite.

**Memory + knowledge — tabelas SQLite (Wave 6):**

| Tabela | Colunas | FTS5 virtual table |
|---|---|---|
| `knowledge_patterns` | `id INTEGER PK, pattern TEXT, confidence REAL, count INTEGER, last_seen TEXT, source TEXT, created_at TEXT` | `knowledge_fts(pattern, source)` (contentless, sync via triggers) |
| `memory_decisions` | `id INTEGER PK, content TEXT, source TEXT, context TEXT, at TEXT` | `memory_decisions_fts(content, source, context)` |
| `memory_lessons` | `id INTEGER PK, content TEXT, source TEXT, context TEXT, at TEXT` | `memory_lessons_fts(content, source, context)` |

Tokenizer FTS5: `unicode61` (case+accent-insensitive, suficiente pro mix PT/EN do Mustard). Triggers `AFTER INSERT/DELETE/UPDATE` sincronizam FTS contentless. Query típica: `SELECT m.* FROM memory_decisions m JOIN memory_decisions_fts f ON m.id = f.rowid WHERE memory_decisions_fts MATCH 'auth OR migration' ORDER BY rank LIMIT 10`.

## Arquivos

- `packages/core/src/model/event.rs` (edição) — constantes `EVENT_PIPELINE_*` + structs serde `PipelineScopePayload`, `PipelineStatusPayload`, `PipelineTaskDispatchPayload`, `PipelineTaskCompletePayload`, `PipelineWaveCompletePayload`, `PipelineDispatchFailurePayload`, `PipelinePausePayload`, `PipelineResumeModePayload`.
- `apps/rt/src/run/emit_pipeline.rs` (novo) — subcomando `mustard-rt run emit-pipeline --kind <name> --spec <name> [--payload <json>]`. Valida kind. Emite via `SqliteEventStore`. Fail-open em store error; exit ≠ 0 em kind desconhecido.
- `apps/rt/src/run/mod.rs` (edição) — `mod emit_pipeline;` + variante `RunCmd::EmitPipeline { kind, spec, payload }`.
- `apps/rt/src/run/event_projections.rs` (edição grande) — `pub struct PipelineStateView { ... }` + `pub fn pipeline_state_for_spec(store: &SqliteEventStore, spec: &str) -> Option<PipelineStateView>`. Fold de eventos: status = último `pipeline.status.to`; scope/lang/model = último `pipeline.scope`; tasks = construído de dispatch + complete; completed_waves = de `pipeline.wave.complete.wave`; current_wave = `max(completed_waves)+1` ou 1; is_wave_plan = FS (presença de `wave-plan.md`) OR `pipeline.scope.is_wave_plan`; last_dispatch_failure = último `pipeline.dispatch_failure` <10min; paused/resume_mode = últimos respectivos.
- `apps/rt/src/hooks/close_gate.rs` (edição) — `extract_phase` foi migrado pela spec-mãe; demais leituras de state → projeção.
- `apps/rt/src/hooks/path_guard.rs` (edição) — leituras de `state.get(...)` → projeção.
- `apps/rt/src/hooks/post_edit.rs` (edição) — leituras → projeção. Remover qualquer write residual de pipeline-state.
- `apps/rt/src/run/epic_fold.rs` (edição) — `state_phase` (já migrado) e demais reads → projeção. Stop de write de pipeline-state-style fields no epic-state JSON (epic-state propriamente dito é arquivo separado e fica).
- `apps/rt/src/run/statusline.rs` (edição) — read via projeção.
- `apps/dashboard/src-tauri/src/db.rs` (edição) — `pub fn pipelines_from_db(conn) -> Vec<PipelineSummary>` espelhando `specs_from_db`. SQL agrupado por spec usando os events novos. Index `idx_events_spec` em events (já talvez exista; verificar).
- `apps/dashboard/src-tauri/src/lib.rs` (edição grande) — 
  - `specs_from_fs`: deixa de walking `.claude/.pipeline-states/*.json`. Continua walking `spec/active/**/spec.md` + `wave-plan.md` pra discovery e parse de frontmatter (title, lang, scope).
  - `dashboard_pipelines`: troca walk de pipeline-states por `db::pipelines_from_db(conn)`.
  - `dashboard_active_pipelines`: idem.
  - `dashboard_specs::merge`: DB ganha pra todos os campos derivados de eventos (status, tasks_count, current_wave, phase); FS ganha pra title/spec frontmatter.
- `apps/cli/templates/commands/mustard/feature/SKILL.md` (edição) — substituir cada Write/Edit de pipeline-state por `mustard-rt run emit-pipeline --kind <evt>`. `wave-plan.md` continua sendo escrito (narrativa).
- `apps/cli/templates/commands/mustard/approve/SKILL.md` (edição) — Step 5 "Pipeline State" → `emit-pipeline --kind status --payload '{"from":"draft","to":"approved"}'`.
- `apps/cli/templates/commands/mustard/resume/SKILL.md` (edição grande) — 
  - Step 0 Dispatch Failure Pre-Check: query `last_dispatch_failure` via projeção, não mais read JSON.
  - Step 0.5 Resume Mode: `emit-pipeline --kind resume_mode --payload '{"mode":"reanalyzed"}'`.
  - Step 9-10: `emit-pipeline --kind status --payload '{"from":"approved","to":"implementing"}'`.
  - Step 12c wave transitions: `emit-pipeline --kind wave.complete --payload '{"wave":N,"duration_ms":...}'`.
  - Step 17 dispatch: `emit-pipeline --kind task.dispatch` antes; `--kind task.complete` depois.
- `apps/cli/templates/commands/mustard/close/SKILL.md` (edição) — `emit-pipeline --kind status --payload '{"from":"implementing","to":"completed"}'`. Move spec dir + delete pipeline-state JSON (Step 20) vira só "Move spec dir" (não há mais JSON pra deletar pós migração); legacy delete fica como one-shot residual durante transição.
- `apps/cli/templates/commands/mustard/qa/SKILL.md` (edição) — limpar menções a `phaseName` (débito de Wave 2 da spec-mãe, AC-6 da espec-mãe não incluía esta) + qualquer Write de pipeline-state. QA continua emitindo `qa.result`.
- `apps/cli/templates/commands/mustard/bugfix/SKILL.md` (edição) — idem.
- `apps/cli/templates/pipeline-config.md` (edição) — Shared Memory Architecture: row `.pipeline-states/{spec}.json` removida da "Persistent projections" table; narrativa: "estado derivado integralmente de eventos via `pipeline_state_for_spec`".
- `apps/dashboard/CLAUDE.md` (edição) — Shared memory section: refresh narrativo equivalente.
- `apps/rt/src/run/pipeline_state_ingest.rs` (novo) — subcomando one-shot `mustard-rt run pipeline-state-ingest [--delete]`. Globa `.claude/.pipeline-states/*.json` (ignora `*.metrics.json`); pra cada arquivo emite events retroativos com `at` derivado de `updatedAt`; opcionalmente deleta o arquivo após ingest. Saída JSON `{ ingested, deleted, errors }`. Fail-open em parse error.
- `apps/rt/tests/pipeline_state_projection_test.rs` (novo) — testa fold de eventos → `PipelineStateView` (caso happy, caso degenerado sem eventos, conflict resolution entre events).
- `apps/dashboard/src-tauri/tests/pipelines_from_db_test.rs` (novo) — emite sequência de events, espera `PipelineSummary` correto.
- `.claude/.docs-audit.json` (edição) — entry nova: `from_spec: 2026-05-19-pipeline-state-from-sqlite`, `obsolete_terms: ["\\.pipeline-states/.*\\.json", "lastDispatchFailure(?!_in_event)", "pipelineState\\.write", "knowledge\\.json", "memory/decisions\\.json", "memory/lessons\\.json"]`, hint apontando pra projeção / tabela.
- `packages/core/src/io/sqlite_store.rs` (edição grande) — schema migration: cria 3 tabelas (`knowledge_patterns`, `memory_decisions`, `memory_lessons`) + 3 virtual tables FTS5 + 9 triggers de sync. Idempotente (`CREATE TABLE IF NOT EXISTS` + `CREATE VIRTUAL TABLE IF NOT EXISTS`). Versão de schema bumped pra coordenar migrations.
- `apps/rt/src/run/memory.rs` (edição) — subcomando `mustard-rt run memory <decision|lesson>` deixa de escrever em `memory/*.json`; passa a `INSERT INTO memory_decisions` / `memory_lessons`. Interface CLI inalterada (mesmos flags). Output JSON também inalterado.
- `apps/rt/src/run/knowledge.rs` ou hook equivalente (edição) — quem hoje escreve `knowledge.json` (provavelmente `apps/rt/src/hooks/knowledge.rs` no `knowledge` module per pipeline-config.md) passa a `INSERT INTO knowledge_patterns ON CONFLICT(pattern) DO UPDATE SET confidence = ..., count = count+1, last_seen = ...`.
- `apps/rt/src/hooks/session_start.rs` (edição) — bootstrap que hoje lê `knowledge.json` + `memory/decisions.json` + `memory/lessons.json` para injetar no contexto inicial; passa a `SELECT ... FROM ... ORDER BY ... LIMIT N` (top-N por confidence/recency).
- `apps/rt/src/hooks/pre_compact.rs` (se existir; verificar) — mesma migração de leitura.
- `apps/rt/src/run/memory_ingest.rs` (novo) — subcomando `mustard-rt run memory-ingest [--delete]`. Lê `.claude/knowledge.json`, `.claude/memory/decisions.json`, `.claude/memory/lessons.json` se existirem; pra cada entry faz `INSERT` na tabela correspondente; preserva `at`/`created_at` quando presente; opcionalmente deleta os JSONs após ingest sucesso. Saída JSON `{ ingested: { knowledge: N, decisions: M, lessons: K }, deleted: bool, errors: [] }`. Fail-open.
- `apps/dashboard/src-tauri/src/lib.rs` ou módulo dedicado (edição) — se o dashboard tem aba/comando que lê `knowledge.json` ou `memory/*.json`, migra pra query SQLite. Grep pra confirmar (`knowledge.json`, `memory/decisions`, `memory/lessons`).
- `apps/cli/templates/pipeline-config.md` (edição na Shared Memory Architecture já listada acima) — atualizar rows `knowledge.json` + `memory/decisions.json` + `memory/lessons.json` da "Persistent projections" table: Writer column passa pra "INSERT direto via `mustard-rt run memory`/hook knowledge"; Purpose preservado.
- `apps/rt/tests/memory_sqlite_test.rs` (novo) — testa que `mustard-rt run memory decision` insere na tabela; query FTS5 retorna resultado.
- `packages/core/tests/sqlite_fts5_smoke.rs` (novo) — smoke test FTS5: cria tabela, insere 3 entries, `MATCH` retorna esperado.

## Tarefas

### Wave 1 — Event constants + `emit-pipeline` subcommand (mustard-core + mustard-rt)

- [ ] Em `packages/core/src/model/event.rs`: adicionar `pub const EVENT_PIPELINE_SCOPE: &str = "pipeline.scope"`, `EVENT_PIPELINE_STATUS`, `EVENT_PIPELINE_TASK_DISPATCH`, `EVENT_PIPELINE_TASK_COMPLETE`, `EVENT_PIPELINE_WAVE_COMPLETE`, `EVENT_PIPELINE_DISPATCH_FAILURE`, `EVENT_PIPELINE_PAUSE`, `EVENT_PIPELINE_RESUME_MODE`. Adicionar structs serde para typed payload de cada (lenient: `#[serde(default)]` em campos novos).
- [ ] Em `apps/rt/src/run/emit_pipeline.rs` (novo): subcomando que aceita `--kind <name> --spec <name> [--payload <json>]`. Valida kind contra a tabela; emite via `SqliteEventStore::append()`. Mirror do padrão `emit_phase`/`emit_event`.
- [ ] Em `apps/rt/src/run/mod.rs`: registrar `pub mod emit_pipeline;` + variante `RunCmd::EmitPipeline { kind: String, spec: String, payload: Option<String> }` no enum. Dispatch arm chama `emit_pipeline::run(opts)`.
- [ ] Testes: cada kind aceita seu payload e dispara `append`. Kind desconhecido → exit 1. Payload inválido (não-JSON) → exit 1. Store error → fail-open (exit 0, stderr quiet).
- [ ] Validate: `cargo build -p mustard-core -p mustard-rt` + `cargo test -p mustard-rt`.

### Wave 2 — Projeção `pipeline_state_for_spec` (mustard-rt)

- [ ] Em `apps/rt/src/run/event_projections.rs`: adicionar `pub struct PipelineStateView` com campos snake_case espelhando o JSON antigo (`status, scope, lang, model, is_wave_plan, total_waves, current_wave, completed_waves, tasks, last_dispatch_failure, paused_at, pause_reason, resume_mode`). Cada campo `Option<T>` quando sem evento correspondente.
- [ ] `pub fn pipeline_state_for_spec(store: &SqliteEventStore, spec: &str) -> Option<PipelineStateView>`. Fold algorítmico per § Informações da Entidade.
- [ ] Garantir index `CREATE INDEX IF NOT EXISTS idx_events_spec ON events(spec)` (verificar se já existe; senão adicionar via migration helper).
- [ ] Testes: pra cada campo, sequência de eventos → projeção esperada. Spec sem eventos → `None`. Eventos conflitantes (e.g. 2 `pipeline.status` com `to` diferentes) → último vence (ORDER BY id DESC).
- [ ] Re-export em `apps/rt/src/run/mod.rs` se for consumido fora de event_projections (sim — hooks vão consumir).
- [ ] Validate: `cargo test -p mustard-rt`.

### Wave 3 — Migrar readers (hooks rt + dashboard) — depende de Wave 2

- [ ] `apps/rt/src/hooks/close_gate.rs`: substituir leitura de `.pipeline-states/{spec}.json` por `pipeline_state_for_spec(store, spec)`. `extract_phase` foi migrado pela spec-mãe (mantido como dead-code defensive); demais leituras de state → projeção. Fail-open: projeção `None` → `Verdict::Allow`.
- [ ] `apps/rt/src/hooks/path_guard.rs`: substituir `state.get(...)` por field da `PipelineStateView`. Extrair `spec` do path do file_path do tool ou do JSON content que vier no input.
- [ ] `apps/rt/src/hooks/post_edit.rs`: leituras → projeção. Remover qualquer write residual de pipeline-state (a spec-mãe deletou `run_pipeline_phase`; verificar se há outros).
- [ ] `apps/rt/src/run/epic_fold.rs`: `state_phase` (migrado) e demais leituras → projeção. Stop de write de pipeline-state-style fields no epic-state JSON. Epic-state propriamente dito fica como arquivo (não é pipeline-state).
- [ ] `apps/rt/src/run/statusline.rs`: read via projeção.
- [ ] `apps/dashboard/src-tauri/src/db.rs`: `pub fn pipelines_from_db(conn) -> Vec<PipelineSummary>`. SQL agrupado por spec usando events. Index garantido em Wave 2.
- [ ] `apps/dashboard/src-tauri/src/lib.rs`:
  - `specs_from_fs`: deletar walking de `.claude/.pipeline-states/*.json`. Manter walking de `spec/active/**/spec.md` + `wave-plan.md` para discovery + frontmatter.
  - `dashboard_pipelines`: replace walk → `db::pipelines_from_db(conn)`.
  - `dashboard_active_pipelines`: replace walk → `db::pipelines_from_db(conn)` filtered por status != completed.
  - `dashboard_specs::merge`: DB wins pra status, tasks_count, current_wave (alem de phase já migrado); FS wins pra title/spec.md frontmatter.
- [ ] Testes:
  - rt: cobertos em Wave 2 pra projeção; novo teste por hook que valida `Verdict::Allow` em projection `None`.
  - dashboard: novo `pipelines_from_db_test.rs` — emite events, espera summary.
- [ ] Validate: `cargo build -p mustard-rt -p mustard-dashboard` + `cargo test -p mustard-rt -p mustard-dashboard` + `pnpm --filter mustard-dashboard build`.

### Wave 4 — Migrar writers (SKILL.md de pipeline) — depende de Wave 3

- [ ] `apps/cli/templates/commands/mustard/feature/SKILL.md`:
  - Step que cria pipeline-state JSON → substituir por `mustard-rt run emit-pipeline --kind scope --spec X --payload '{"scope":"full","lang":"pt","model":"opus","is_wave_plan":false}'` e `--kind status --payload '{"from":null,"to":"draft"}'`.
  - Wave-plan scaffold continua criando `wave-plan.md`; state de waves (currentWave=1, completedWaves=[]) é derivado de events (zero events `pipeline.wave.complete` → currentWave=1, completedWaves=[]).
  - Remover qualquer Write/Edit de `.pipeline-states/{spec}.json`.
- [ ] `apps/cli/templates/commands/mustard/approve/SKILL.md`: Step 5 "Pipeline State" → `emit-pipeline --kind status --payload '{"from":"draft","to":"approved"}'`. Step 5b decisions continua via `mustard-rt run memory decision`. Step 7 TaskCreate inalterado.
- [ ] `apps/cli/templates/commands/mustard/resume/SKILL.md`:
  - Step 0 Dispatch Failure: query via projection (sem read JSON).
  - Step 0.5 Resume Mode: `emit-pipeline --kind resume_mode --payload '{"mode":"reanalyzed"}'` (ou "continued"/"escalated").
  - Step 9-10: `emit-pipeline --kind status --payload '{"from":"approved","to":"implementing"}'`.
  - Step 12c wave transitions: `emit-pipeline --kind wave.complete --payload '{"wave":N,"duration_ms":...}'`.
  - Step 17 dispatch: `emit-pipeline --kind task.dispatch` antes da chamada Task; `--kind task.complete` após retorno.
  - Pause Handoff: `emit-pipeline --kind pause --payload '{"reason":"...","next_action":"..."}'`.
- [ ] `apps/cli/templates/commands/mustard/close/SKILL.md`: `emit-pipeline --kind status --payload '{"from":"implementing","to":"completed"}'`. Move spec dir mantido (narrativa). Delete pipeline-state JSON vira no-op (não há mais JSON pra deletar pós Wave 5 ingest).
- [ ] `apps/cli/templates/commands/mustard/qa/SKILL.md`: limpar menções a `phaseName` (débito da spec-mãe não incluído em AC-6 daquela spec) + substituir qualquer Write de pipeline-state. `qa.result` continua sendo emitido (já é evento canônico).
- [ ] `apps/cli/templates/commands/mustard/bugfix/SKILL.md`: idem.
- [ ] `apps/cli/templates/pipeline-config.md`: Shared Memory Architecture — remover row `.pipeline-states/{spec}.json` da "Persistent projections" table. Narrativa: "estado derivado integralmente de eventos via `pipeline_state_for_spec`".
- [ ] `apps/dashboard/CLAUDE.md`: Shared memory section refresh equivalente.
- [ ] Validate: `cargo build -p mustard-rt -p mustard-cli` + dashboard build (narrativas não derrubam builds; validação cosmética).

### Wave 6 — Memory + knowledge em SQLite com FTS5 — pode rodar paralelo a Waves 3-4 (toca arquivos disjuntos)

- [ ] Em `packages/core/src/io/sqlite_store.rs`: adicionar schema migration na função de init (ou via versioning helper se já existir). Cria 3 tabelas (`knowledge_patterns`, `memory_decisions`, `memory_lessons`) + 3 virtual tables FTS5 (`knowledge_fts`, `memory_decisions_fts`, `memory_lessons_fts`) + triggers `AFTER INSERT/DELETE/UPDATE` por tabela pra sincronizar contentless FTS. Idempotente. Verificar antes que `bundled-full` (ou equivalente) está no feature set do `libsqlite3-sys` para FTS5 vir habilitado — se não, adicionar.
- [ ] `apps/rt/src/run/memory.rs`: substituir append em `memory/*.json` por `INSERT INTO memory_{decisions,lessons}`. CLI (`--type`/`--content`/`--source`/`--context`) inalterada. Output JSON do subcommand inalterado. Fail-open em store error (não rompe pipeline).
- [ ] Localizar quem escreve `knowledge.json` (Grep `apps/rt/src/` por `knowledge.json`); migrar para `INSERT INTO knowledge_patterns ON CONFLICT(pattern) DO UPDATE SET confidence = ?, count = count + 1, last_seen = ?`. Provavelmente `apps/rt/src/hooks/knowledge.rs` ou hook em `mod.rs` no event SessionEnd / PostToolUse(Task).
- [ ] Migrar `apps/rt/src/hooks/session_start.rs` (bootstrap injection): hoje lê os 3 JSONs; passa a `SELECT ... LIMIT N ORDER BY confidence DESC` (knowledge) e `SELECT ... ORDER BY at DESC LIMIT N` (memory). Caps de injection mantidos (400-800 chars per pipeline-config.md).
- [ ] Migrar `apps/rt/src/hooks/pre_compact.rs` se existir e ler memory/knowledge (Grep confirma).
- [ ] `apps/rt/src/run/memory_ingest.rs` (novo): subcomando one-shot `mustard-rt run memory-ingest [--delete]`. Lê JSONs existentes, `INSERT` em batch nas tabelas, preserva timestamps, opcionalmente deleta os arquivos. Saída JSON. Fail-open em parse error por arquivo.
- [ ] Registrar `mod memory_ingest;` + variante `RunCmd::MemoryIngest { delete: bool }` em `apps/rt/src/run/mod.rs`.
- [ ] Rodar `mustard-rt run memory-ingest --delete` no repo Mustard para limpar os 3 JSONs em flight.
- [ ] Migrar consumidores no dashboard (sites confirmados via Grep):
  - **`apps/dashboard/src-tauri/src/lib.rs` (em ~linha 269)** — Tauri command (provavelmente `dashboard_knowledge`) que lê `knowledge.json` pra retornar `KnowledgeSummary { patterns_count, conventions_count, high_confidence_count }`. Migrar pra `SELECT COUNT(*) FROM knowledge_patterns WHERE ...`. **Preservar shape do struct** — `Knowledge.tsx` (frontend) consome esse command e quebra se mudar.
  - **`apps/dashboard/src-tauri/src/watcher.rs:40-42`** — file watcher detecta mudanças em `knowledge.json`/`memory/decisions.json`/`memory/lessons.json` pra empurrar update no frontend via Tauri event. Pós-migração esses arquivos não mudam (deletados em Wave 5 ingest). **Decisão (a):** remover os 2 ramos do watcher pra esses paths; frontend `Knowledge.tsx` passa a refetchar via TanStack Query `refetchOnWindowFocus: true` + `refetchInterval: 10_000`. Mais simples que emitir Tauri events a partir de INSERT em tabela. Documentar no commit que "live update" passa de fs-push pra polling-pull (perda mínima dado UX da página).
  - **`apps/dashboard/src/pages/Knowledge.tsx`** — adicionar `refetchOnWindowFocus: true` + `refetchInterval` no `useQuery` que consome o command migrado (substituindo a invalidação que vinha do watcher). Sem mudança de shape — Tauri command preserva contrato.
- [ ] Testes:
  - `packages/core/tests/sqlite_fts5_smoke.rs` (novo): cria DB temp, insere 3 entries em `memory_decisions`, `MATCH` retorna 1 com termo seed. Valida FTS5 está disponível no build.
  - `apps/rt/tests/memory_sqlite_test.rs` (novo): `mustard-rt run memory decision` insere no DB, query confirma.
- [ ] Atualizar `apps/cli/templates/pipeline-config.md` "Persistent projections" table: rows de knowledge/memory passam pra "INSERT direto em tabela SQLite (`knowledge_patterns`/`memory_decisions`/`memory_lessons`) com FTS5".
- [ ] `cargo build -p mustard-core -p mustard-rt && cargo test -p mustard-core -p mustard-rt`.

### Wave 5 — Legacy ingest + cleanup + docs-audit — depende de Wave 4 + Wave 6

- [ ] `apps/rt/src/run/pipeline_state_ingest.rs` (novo): subcomando `mustard-rt run pipeline-state-ingest [--delete]`. Globa `.claude/.pipeline-states/*.json` (ignora `*.metrics.json`). Pra cada arquivo:
  - Parse JSON com lenient serde.
  - Emite events retroativos com `at` baseado em `updatedAt` (ou `createdAt` se ausente): `pipeline.scope` (de `scope`/`lang`/`model`/`isWavePlan`/`totalWaves`); `pipeline.status` (de `status`); `pipeline.task.dispatch` por entrada `tasks[]` (com `wave`, `agent`, `files`); `pipeline.task.complete` pras tasks já marcadas completed; `pipeline.wave.complete` por entrada de `completedWaves[]`; `pipeline.dispatch_failure` se `lastDispatchFailure` presente; `pipeline.pause` se `pausedAt` presente.
  - Se `--delete`, remove arquivo após ingest sucesso.
  - Saída JSON: `{ ingested: N, deleted: M, errors: [{file, error}] }`.
- [ ] Registrar em `apps/rt/src/run/mod.rs`.
- [ ] Rodar `mustard-rt run pipeline-state-ingest --delete` no repo Mustard para limpar JSONs em flight (inclui o da spec `artifact-update-followups` que está active). Validar que `/resume` daquela spec continua funcionando pós-ingest.
- [ ] Adicionar audit em `.claude/.docs-audit.json`:
  ```json
  {
    "from_spec": "2026-05-19-pipeline-state-from-sqlite",
    "closed_at": "<close date>",
    "obsolete_terms": [
      "\\.pipeline-states/.*\\.json",
      "lastDispatchFailure(?!_in_event)",
      "pipelineState\\.write"
    ],
    "replacement_hint": "Estado de pipeline derivado integralmente de eventos SQLite via pipeline_state_for_spec projection; emissores usam mustard-rt run emit-pipeline"
  }
  ```
- [ ] Dogfood: `mustard-rt run docs-stale-check --from 2026-05-19-pipeline-state-from-sqlite` → `hits: []` (assumindo Wave 4 narrative refresh limpou as menções source-of-truth em CLAUDE.md/pipeline-config.md).
- [ ] Validate: `cargo build && cargo test -p mustard-rt -p mustard-dashboard` + dashboard build + AC-7/8/9 (sem Write/.pipeline-states em SKILL.md / lib.rs / hooks).

## Dependências

- **Spec ascendente:** `2026-05-19-dashboard-phase-from-sqlite` (CLOSE 2026-05-20) — estabeleceu o padrão (DB-wins per derived field, projection helper, gate inline em emit). Esta finaliza o trabalho.
- **Prerequisite implícito de:** `2026-05-19-artifact-update-followups` Wave 3 — surface de artefatos no dashboard pressupõe state reading consistente. Esta deve fechar antes daquela Wave 3.
- **Pré-requisito de fase futura:** sync layer (ElectricSQL / PowerSync / Litestream). Sem source-of-truth única, replicação fica inconsistente. Spec separada quando essa fechar.

## Limites

- `packages/core/src/model/event.rs` — constantes + payload structs novos
- `apps/rt/src/run/` — `emit_pipeline.rs` (novo), `event_projections.rs` (extensão grande), `pipeline_state_ingest.rs` (novo), `mod.rs` (registros)
- `apps/rt/src/hooks/` — `close_gate.rs`, `path_guard.rs`, `post_edit.rs` (readers only)
- `apps/rt/src/run/` — `epic_fold.rs` (mixed read+write), `statusline.rs` (reader)
- `apps/dashboard/src-tauri/src/` — `db.rs` (nova função), `lib.rs` (3 funções migram)
- `apps/cli/templates/commands/mustard/{feature,approve,resume,close,qa,bugfix}/SKILL.md` — writers
- `apps/cli/templates/pipeline-config.md` — Shared Memory section apenas
- `apps/dashboard/CLAUDE.md` — Shared memory section apenas
- `.claude/.docs-audit.json` — entry nova
- `apps/rt/tests/`, `apps/dashboard/src-tauri/tests/`, `packages/core/tests/` — novos testes
- `packages/core/src/io/sqlite_store.rs` — schema migration (3 tabelas + 3 FTS5 virtual + 9 triggers)
- `apps/rt/src/run/memory.rs` + `apps/rt/src/run/memory_ingest.rs` — writers + ingest one-shot
- `apps/rt/src/hooks/session_start.rs` (+ `pre_compact.rs` se aplicável) — readers de memory/knowledge migram pra SELECT
- `apps/rt/src/hooks/knowledge.rs` (ou módulo equivalente) — writer de knowledge migra pra INSERT
- `apps/dashboard/src-tauri/src/lib.rs` (no Tauri command de KnowledgeSummary, ~linha 269) — reader migra pra SELECT; shape preservado
- `apps/dashboard/src-tauri/src/watcher.rs` (linhas 40-42) — remove 2 ramos (knowledge/memory paths não mudam mais)
- `apps/dashboard/src/pages/Knowledge.tsx` — adiciona refetchOnWindowFocus + refetchInterval no useQuery (substitui live-update do watcher)
- **Fora dos limites:** `spec.md`/`wave-plan.md` (narrativa fica); `CLAUDE.md`/`pipeline-config.md` fora de Shared Memory; `entity-registry.json` (cache de scan); `mustard.json` (config user); `.docs-audit.json` (config — exceto entry nova dessa spec); `recipes/*.json` (templates human-edited); `.pipeline-states/*.metrics.json` (cleanup separado); sync layer (próxima fase pós-essa); busca vetorial / `sqlite-vec` (spec futura quando volume justificar).
