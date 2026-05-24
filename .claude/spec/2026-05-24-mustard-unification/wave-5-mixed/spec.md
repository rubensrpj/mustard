# W5 — mustard.db redesign + per-spec event log + dashboard fast/lean

### Stage: Plan
### Outcome: Active
### Phase: PLAN
### Scope: full
### Checkpoint: 2026-05-24T19:30:00Z
### Lang: pt-BR
### Parent: 2026-05-24-mustard-unification

## Contexto

Três problemas convergem nesta onda:

1. Tabela `events` (com `events_fts` FTS5) do `mustard.db` cresce indefinidamente; cada hook escreve via SQLite open + INSERT + commit (~100-500 µs); multi-agentes em waves paralelas brigam pelo lock único.
2. Schema do `mustard.db` em si carrega legado JS (`knowledge` separada de `knowledge_patterns`, `metrics_projection` duplicada com `run_usage` do `telemetry.db`) e índices não auditados. Solução: redesenhar do zero (CREATE direto), não `ALTER COLUMN` paliativo.
3. Página `/specs` do dashboard ficou lenta — render do grafo interno é o gargalo e duplica o vault Obsidian que o `mustard-rt` já gera. Solução: remover grafo interno; wikilinks abrem no Obsidian via `obsidian://` URI scheme.

Esta onda absorve integralmente a spec ativa `2026-05-23-per-spec-event-log-claude-devtools` (5 ondas originais viram T5.1..T5.5) + adiciona 3 tasks novas (T5.6 schema, T5.7 dashboard, T5.8 economy events).

## Tarefas

- [ ] **T5.1 (W1 original — library, rt writer).** Novo `EventSink` por NDJSON append em `.claude/spec/{name}/[wave-N-{role}/]events/{ts-ns}-{run-id}-{pid}.ndjson`. Blob spill content-addressed em `blobs/{ab}/{sha256}.bin` (threshold 4 KB — transcript de Task, Read full file, Bash output longo). Mini-tabela `pipeline_events` em `mustard.db` para eventos de ciclo de vida. Drop limpo de `events` + `events_fts`. Campo `parent_id` para recursão de Task. Novo `event_writer_ndjson.rs` + `blob_spill.rs` em `apps/rt/src/run/`.
- [ ] **T5.2 (W2 original — library, core reader).** Substituir `packages/core/src/projection/timeline.rs` para ler NDJSON; atualizar `model/view/timeline.rs` com shape novo (`input`, `output`, `tokens_in`, `tokens_out`, `duration_ms`, `parent_id`); reescrever `mustard-rt run rebuild-specs` para consumir NDJSON.
- [ ] **T5.3 (W3 original — ui, dashboard timeline claude-devtools-style).** Rewrite `SpecTimelineTab` e `PipelineTimeline` — lista flat de tool calls (ícone, label, tokens in/out, duração, status dot); expand revela `Input`/`Output` renderizados por tool (Bash → terminal, Read → Code/Preview toggle, Edit → diff, Glob/Grep → lista, Task → execution trace recursiva via `parent_id`). Live tail via `notify-rs` no `src-tauri` emite eventos Tauri que React consome em tempo real. Profile React DevTools alvo `<16ms` re-render em wave com 500+ eventos.
- [ ] **T5.4 (W4 original — general, sessions).** Nova tabela `sessions` em `mustard.db` + diretório `.claude/.session/{slug}/events/` para sessões sem spec ativa. Rota `Sessions.tsx` no dashboard com sidebar.
- [ ] **T5.5 (W5 original — general, spec-clear).** `mustard-rt run spec-clear` + `/mustard:spec clear` slash command. Flags `--dry-run` (default), `--apply`, `--all`, `--name`, `--age-days` (default 15). Algoritmo: glob `.claude/spec/*/spec.md` → parse `meta.json` (W3 garantiu) → filtra Close+Done → para cada, lê mtime mais recente em `events/` recursivo → compara com cutoff → emite linha na tabela ou apaga.
- [ ] **T5.6 (NOVA — mustard.db schema redesign).** Reescrever `packages/core/src/store/sqlite_schema.sql` do zero:
  - **Drop**: `events`, `events_fts` (já em T5.1); `knowledge` legacy JS-era (consolida em `knowledge_patterns`); `metrics_projection` (dados duplicam `run_usage` do telemetry.db — query direto).
  - **Manter (lean)**: `pipeline_events` (novo, T5.1), `sessions` (novo, T5.4), `pipeline_amend_window` (já existe), `specs` (cache denormalizado).
  - **CREATE direto no schema final** (não `ALTER ADD COLUMN`): `knowledge_patterns` com `(id, pattern UNIQUE, confidence, count, last_seen, source, created_at, spec, status, last_used)`. `memory_decisions` e `memory_lessons` com `(id, content, source, context, at, spec, wave, confidence, status, superseded_by)` + FTS5 mirrors. `agent_memory` + `memory_feedback` (W8 antecipa apenas o DDL aqui; lógica fica em W8).
  - **Índices auditados**: `EXPLAIN QUERY PLAN` em cada query consumidora; criar índice se hit full-scan; documentar lista final no header do schema.
  - `VACUUM` ao final de `apply_schema` + `PRAGMA optimize` no `open()`.
  - Benchmark: tamanho do db em projeto canário pré (X MB) vs pós (Y MB); reportar via evento `pipeline.economy.schema.shrunk { from_bytes, to_bytes }`.
- [ ] **T5.7 (NOVA — dashboard performance + obsidian wikilinks).**
  - Remover componente de grafo interno da página `/specs` (auditar `apps/dashboard/src/` — provavelmente `react-force-graph`, `d3-force`, ou `vis-network`). Remover dependência do `package.json`.
  - Wikilinks `[[X]]` no dashboard abrem via `obsidian://open?vault=<vault-name>&file=<relative-path>` ou Tauri `shell.open` equivalente. Vault path vem de `mustard.json#obsidianVault` (novo campo, default `.claude/.obsidian`).
  - Profile página `/specs`: tempo até interativa `<200ms` para vault com 100 specs.
  - Cache de spec list: `@tanstack/react-query` (ou equivalente) com `staleTime`; invalidação via `notify-rs` event quando `meta.json` muda.
  - Página `/specs` final: lista virtualizada (`@tanstack/react-virtual` ou `react-virtuoso`) + search + filter + sort. Sem visualização de grafo.
- [ ] **T5.8 (NOVA — economy events from event-log).** EventSink novo de T5.1 emite `pipeline.economy.event.written { duration_ns, bytes_written, spilled_to_blob: bool }` por escrita. Comparado com baseline SQLite (~100k-500k ns), espera-se ~30k ns. Visível em `/economia` (W12).

## Files

- `packages/core/src/store/sqlite_schema.sql` (T5.1 + T5.6 — refeito do zero)
- `packages/core/src/store/sqlite_store.rs` (T5.1 — EventSink NDJSON)
- `packages/core/src/projection/timeline.rs` (T5.2)
- `packages/core/src/model/view/timeline.rs` (T5.2)
- `apps/rt/src/run/event_writer_ndjson.rs` (novo, T5.1)
- `apps/rt/src/run/blob_spill.rs` (novo, T5.1)
- `apps/rt/src/run/rebuild_specs.rs` (T5.2)
- `apps/rt/src/run/spec_clear.rs` (novo, T5.5)
- `apps/rt/src/run/mod.rs` (registrar novos)
- `apps/cli/templates/commands/mustard/spec/SKILL.md` (T5.5 — adicionar subcommand `clear`)
- `apps/dashboard/src/components/SpecTimelineTab.tsx` + `PipelineTimeline.tsx` + per-tool renderers + `Sessions.tsx` (T5.3, T5.4)
- `apps/dashboard/src/pages/Specs.tsx` (T5.7 — sem grafo, virtualizada)
- `apps/dashboard/package.json` (T5.7 — remover dep força-grafo)
- `apps/dashboard/src-tauri/src/main.rs` (T5.3 notify-rs + T5.7 shell.open)
- `mustard.json` (T5.7 — campo `obsidianVault`)

## Critérios de Aceitação

- [ ] **AC-5.1.** Tabela `events` dropada do schema. Command: `node -e "const t=require('fs').readFileSync('packages/core/src/store/sqlite_schema.sql','utf8');if(/CREATE TABLE events\\b/.test(t))process.exit(1)"`
- [ ] **AC-5.2.** Tabelas `pipeline_events` e `sessions` existem no schema. Command: `node -e "const t=require('fs').readFileSync('packages/core/src/store/sqlite_schema.sql','utf8');for(const k of ['CREATE TABLE pipeline_events','CREATE TABLE sessions']){if(!t.includes(k))process.exit(1)}"`
- [ ] **AC-5.3.** `.claude/spec/{ativa}/events/*.ndjson` existe após pipeline rodar. Command: `node -e "const fs=require('fs');const p='.claude/spec/2026-05-24-mustard-unification/events';if(!fs.existsSync(p))process.exit(1);if(!fs.readdirSync(p).some(f=>f.endsWith('.ndjson')))process.exit(1)"`
- [ ] **AC-5.4.** Tabela `knowledge` (legacy) e `metrics_projection` dropadas. Command: `node -e "const t=require('fs').readFileSync('packages/core/src/store/sqlite_schema.sql','utf8');for(const k of ['CREATE TABLE knowledge\\\\b','CREATE TABLE metrics_projection']){if(new RegExp(k).test(t))process.exit(1)}"`
- [ ] **AC-5.5.** `package.json` do dashboard não tem dep de força-grafo. Command: `node -e "const j=JSON.parse(require('fs').readFileSync('apps/dashboard/package.json','utf8'));for(const k of ['react-force-graph','react-force-graph-2d','d3-force','vis-network']){if(j.dependencies?.[k]||j.devDependencies?.[k])process.exit(1)}"`
- [ ] **AC-5.6.** Wikilinks no dashboard chamam `obsidian://` URI. Command: `node -e "const{execSync}=require('child_process');const out=execSync('rg \"obsidian://\" apps/dashboard/src',{encoding:'utf8'}).trim();if(!out)process.exit(1)"`
- [ ] **AC-5.7.** `mustard-rt run spec-clear --help` lista flags `--dry-run`/`--apply`/`--age-days`. Command: `rtk mustard-rt run spec-clear --help 2>&1 | grep -qE "dry-run.*apply.*age-days"`
- [ ] **AC-5.8.** Hot path de evento `<50µs` benchmark. Command: `rtk cargo test -p mustard-rt event_writer_ndjson_hot_path 2>&1 | grep -q "ok"`
- [ ] **AC-5.9.** `mustard.db` em projeto canário `< 1MB`. Command: `node -e "const fs=require('fs');const s=fs.statSync('.claude/.harness/mustard.db').size;if(s>1024*1024){console.error(s);process.exit(1)}"`
- [ ] **AC-5.10.** Página `/specs` `<200ms` tempo até interativa. Verificável manualmente via React DevTools Profiler.

## Notas

- Paralelizável com W4. Bloqueia W6 (subcomandos novos), W7 (templates cuts), W8 (memória).
- Padrão fase-dev: drop limpo, sem migration formal (cf. `feedback_no_migration_dev_phase`).
- T5.6 reescreve o schema do zero — não usar `ALTER ADD COLUMN` para `agent_memory`/`memory_feedback`.
