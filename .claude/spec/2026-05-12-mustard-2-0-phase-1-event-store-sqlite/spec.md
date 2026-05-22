# Mustard 2.0 — Phase 1: Event Store SQLite + Projeções

- **Lang**: ptbr
### Stage: Close
### Outcome: Completed
### Flags: 
- **Checkpoint**: 2026-05-12T19:15:00Z
- **Scope**: Full
- **Type**: feature
- **Model**: opus
- **Depends on**: Phase 0 (Runtime Compat)
- **Unlocks**: Phase 2 (OpenTelemetry), Phase 3 (MCP)

## Summary

Promover `events.jsonl` a fonte única (replay log). Adicionar `.claude/.harness/mustard.db` (SQLite + FTS5 via `bun:sqlite`, fallback `better-sqlite3` em Node) com projeções denormalizadas regeneráveis a qualquer momento. Eliminar todos os schemas duplicados (`.pipeline-states/*.metrics.json`, `agentAttempts`, `subagent-registry.json`). Hooks consomem via classe `EventStore` tipada.

## Problem

Mustard tem **11+ stores de estado**, muitos sobrepostos:

- `events.jsonl` (truth post-Wave 4)
- `.pipeline-states/*.json` (live state)
- `.pipeline-states/*.metrics.json` (sidecar morto)
- `metrics/*.json` (novo, mas duplicado com sidecar)
- `.agent-state/`, `.agent-memory/`, `.subagent-registry.json`
- `memory/decisions.json`, `memory/lessons.json`
- `knowledge.json`
- `.detect-cache.json`

**6 bugs** dessa sessão derivaram disso (banner falso, métricas não persistidas, dashboard lendo lugar errado, agent.stop vazio, retry keyword-based, agentAttempts morto). Single-source-of-truth elimina classe inteira de bugs.

Hoje cada hook re-implementa parse de `events.jsonl` (`O(n)` scan). FTS5 dá `O(log n)` indexado. Query top-5 knowledge cai de 5-15ms (JSON parse 35KB) pra <1ms.

## Goal

`.claude/.harness/mustard.db` é projeção indexada de `events.jsonl`. Classe `EventStore` é a única forma de ler/escrever. Schemas mortos deletados. Migration one-shot reidrata DB de events.jsonl (idempotent).

## Acceptance Criteria

1. **EventStore class compila e exporta API tipada**
   ```bash
   bunx tsc --noEmit -p src/runtime/tsconfig.json
   ```
   Sem erros. `EventStore` exporta `append()`, `query()`, `search()`, `rebuild()`.

2. **Schema SQLite criado**
   ```bash
   node -e "const s=require('./dist/runtime/event-store.js'); const e=new s.EventStore('/tmp/test.db'); e.init(); const r=e.tables(); process.exit(r.includes('events')&&r.includes('events_fts')&&r.includes('specs')&&r.includes('metrics_projection')?0:1)"
   ```
   Tables: `events`, `events_fts` (virtual FTS5), `specs`, `metrics_projection`, `knowledge`, `knowledge_fts`.

3. **Migration idempotente do events.jsonl real do sialia**
   ```bash
   bun -e "const fs=require('fs');const path=require('path');const os=require('os');const tmp=path.join(os.tmpdir(),'sialia-harness-qa');fs.rmSync(tmp,{recursive:true,force:true});fs.mkdirSync(tmp,{recursive:true});fs.cpSync('C:/Atiz/Competi/projetos/sialia/.claude/.harness',tmp,{recursive:true});const lines=fs.readFileSync(path.join(tmp,'events.jsonl'),'utf8').split('\n').filter(l=>l.trim());const seen=new Set();for(const l of lines){try{const e=JSON.parse(l);seen.add(e.ts+'|'+(e.sessionId||'')+'|'+e.event+'|'+(e.actor&&e.actor.id||''))}catch(_){}}const expected=seen.size;const {execSync}=require('child_process');execSync('bun dist/migrate/jsonl-to-sqlite.js '+JSON.stringify(tmp),{stdio:'pipe'});execSync('bun dist/migrate/jsonl-to-sqlite.js '+JSON.stringify(tmp),{stdio:'pipe'});const s=require('./dist/runtime/event-store.js');const e=new s.EventStore(path.join(tmp,'mustard.db'));e.init();const c=e.eventCount();console.log('expected:',expected,'actual:',c);process.exit(c===expected?0:1)"
   ```
   Rodar 2x produz exatamente o mesmo count que linhas únicas (chave composta `ts|sessionId|event|actor.id`) em events.jsonl. Conta dinâmica — events.jsonl cresce a cada sessão. Executado sob Bun (EventStore requer `bun:sqlite`).

4. **Query por spec retorna mesmos números que buildPipelineState**
   ```bash
   node tests/integration/event-store-vs-buildpipelinestate.js
   ```
   Compara `EventStore.query({spec}).aggregate()` com `buildPipelineState(events,{spec})` pros 3 specs recuperáveis. Deve ser idêntico.

5. **FTS5 search <5ms em ~1444 events**
   ```bash
   bun -e "const{performance}=require('perf_hooks');const path=require('path');const os=require('os');const s=require('./dist/runtime/event-store.js');const e=new s.EventStore(path.join(os.tmpdir(),'sialia-harness-qa','mustard.db'));e.init();const t=performance.now();const r=e.search('telegram');const d=performance.now()-t;console.log('search took',d.toFixed(2),'ms, results:',r.length);process.exit(d<5?0:1)"
   ```
   <5ms (margem 5x sobre target 1ms). Reusa o DB criado em AC #3 (depende de AC #3 ter rodado antes). Executado sob Bun.

6. **Hooks consomem via EventStore**
   ```bash
   grep -L "readFileSync.*events.jsonl" templates/hooks/*.js templates/hooks/_lib/*.js
   ```
   Zero hooks fazem `readFileSync` direto de events.jsonl. Todos via EventStore.

7. **Schemas mortos deletados**
   ```bash
   test ! -f templates/hooks/__tests__/agent-attempts.test.js && grep -rL "agentAttempts" templates/hooks/ templates/scripts/
   ```
   Sem references a `agentAttempts`. Tests removidos.

8. **Sialia ainda funciona com DB novo**
   ```bash
   cd 'C:/Atiz/Competi/projetos/sialia' && node .claude/scripts/dashboard.js --check
   ```
   Dashboard inicializa, lê DB, retorna `pipelineHealth` consistente com migração.

### Parseable AC (cross-shell, QA-runner)

Comandos cross-shell (cmd.exe + bash) usados pelo `qa-run.js`. Os blocos numerados acima são a versão humana/original.

- [ ] AC-3: migration idempotente do sialia events.jsonl (count dinâmico de unique composite keys) — Command: `bun -e "const fs=require('fs');const path=require('path');const os=require('os');const tmp=path.join(os.tmpdir(),'sialia-harness-qa');fs.rmSync(tmp,{recursive:true,force:true});fs.mkdirSync(tmp,{recursive:true});fs.cpSync('C:/Atiz/Competi/projetos/sialia/.claude/.harness',tmp,{recursive:true});const lines=fs.readFileSync(path.join(tmp,'events.jsonl'),'utf8').split('\n').filter(l=>l.trim());const seen=new Set();for(const l of lines){try{const e=JSON.parse(l);seen.add(e.ts+'|'+(e.sessionId||'')+'|'+e.event+'|'+(e.actor&&e.actor.id||''))}catch(_){}}const expected=seen.size;const {execSync}=require('child_process');execSync('bun dist/migrate/jsonl-to-sqlite.js '+JSON.stringify(tmp),{stdio:'pipe'});execSync('bun dist/migrate/jsonl-to-sqlite.js '+JSON.stringify(tmp),{stdio:'pipe'});const s=require('./dist/runtime/event-store.js');const e=new s.EventStore(path.join(tmp,'mustard.db'));e.init();const c=e.eventCount();process.exit(c===expected?0:1)"`
- [ ] AC-5: FTS5 search 'telegram' under 5ms — Command: `bun -e "const{performance}=require('perf_hooks');const path=require('path');const os=require('os');const s=require('./dist/runtime/event-store.js');const e=new s.EventStore(path.join(os.tmpdir(),'sialia-harness-qa','mustard.db'));e.init();const t=performance.now();const r=e.search('telegram');const d=performance.now()-t;process.exit(d<5?0:1)"`

## Implementation

### Schema SQLite

```sql
-- Append-only event log mirror
CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  session_id TEXT,
  wave INTEGER,
  spec TEXT,
  event TEXT NOT NULL,
  actor_kind TEXT,
  actor_id TEXT,
  payload TEXT  -- JSON
);
CREATE INDEX idx_events_spec ON events(spec);
CREATE INDEX idx_events_event ON events(event);
CREATE INDEX idx_events_ts ON events(ts);

-- FTS5 virtual table (text search across payloads)
CREATE VIRTUAL TABLE events_fts USING fts5(
  event, spec, payload_text,
  content='events', content_rowid='id'
);

-- Triggers to keep FTS in sync
CREATE TRIGGER events_ai AFTER INSERT ON events BEGIN
  INSERT INTO events_fts(rowid, event, spec, payload_text)
  VALUES (new.id, new.event, new.spec, new.payload);
END;

-- Projeções denormalizadas (regeneráveis)
CREATE TABLE specs (
  name TEXT PRIMARY KEY,
  status TEXT,        -- active|closed-followup|completed|cancelled
  phase TEXT,
  started_at TEXT,
  completed_at TEXT,
  affected_files TEXT  -- JSON
);

CREATE TABLE metrics_projection (
  spec TEXT PRIMARY KEY,
  api_calls INTEGER,
  retries INTEGER,
  pass1 INTEGER,  -- bool 0/1
  tool_breakdown TEXT,  -- JSON
  dispatch_failures_by_phase TEXT,  -- JSON
  agent_count INTEGER,
  updated_at TEXT,
  FOREIGN KEY (spec) REFERENCES specs(name)
);

CREATE TABLE knowledge (
  id TEXT PRIMARY KEY,
  type TEXT,    -- pattern|convention|entity
  name TEXT,
  description TEXT,
  confidence REAL,
  created_at TEXT,
  updated_at TEXT,
  source TEXT
);
CREATE VIRTUAL TABLE knowledge_fts USING fts5(name, description, content='knowledge', content_rowid='id');
```

### Classe EventStore (TypeScript)

```typescript
// src/runtime/event-store.ts
import type { Database } from 'bun:sqlite';

export interface EventRecord {
  ts: string; sessionId?: string; wave?: number; spec?: string;
  event: string; actor?: { kind: string; id?: string };
  payload?: Record<string, unknown>;
}

export class EventStore {
  private db: Database;
  constructor(private path: string) {}
  init(): void { /* CREATE TABLES IF NOT EXISTS */ }
  append(ev: EventRecord): void { /* atomic insert + FTS sync via trigger */ }
  query(filter: { spec?: string; event?: string; since?: string }): EventRecord[] {}
  search(text: string): EventRecord[] { /* FTS5 MATCH */ }
  rebuild(): void { /* re-derive projections from events table */ }
  // Projection accessors
  specs(): SpecRecord[] {}
  metrics(spec: string): MetricsRecord | null {}
  knowledge(filter?: { minConfidence?: number; limit?: number }): KnowledgeRecord[] {}
}
```

### Migration script

```typescript
// src/migrate/jsonl-to-sqlite.ts
// Idempotent: lê events.jsonl + JSONs antigos, upsert no DB
// - events.jsonl → events table (skip se ts+sessionId+event já existe)
// - knowledge.json → knowledge + knowledge_fts (upsert by id)
// - metrics/*.json + .pipeline-states/*.metrics.json → metrics_projection (upsert)
// - .pipeline-states/*.json → specs (upsert)
```

### Hooks migration

Cada hook que lê events.jsonl ou pipeline-state passa a:

```javascript
const { EventStore } = require('./_lib/event-store.js'); // generated from TS
const store = new EventStore(path.join(claudeDir, '.harness/mustard.db'));
store.init();  // safe no-op if exists
const events = store.query({ spec: currentSpec });
```

Hooks afetados (busca por `readFileSync.*events.jsonl` no codebase atual):
- `session-memory.js`
- `subagent-tracker.js`
- `metrics-tracker.js`
- `session-knowledge.js` / `-inc`
- `pre-compact.js`

### Schemas a deletar

- ✗ `.pipeline-states/*.metrics.json` (substituído por `metrics_projection` table)
- ✗ Campo `agentAttempts` (substituído por `dispatch_failures_by_phase`)
- ✗ `.subagent-registry.json` (volátil, vai pra in-memory via EventStore.query event='agent.start')

### Dual-write durante transição

Por 1 release: hooks emitem em `events.jsonl` **E** chamam `EventStore.append()`. EventStore lê do DB. Permite rollback. Próximo release: events.jsonl vira só backup (DB é truth).

## Risks

- **Bun:sqlite Windows arestas** → fallback `better-sqlite3` via runtime-shim de Phase 0. Detectado em init.
- **Schema migration em projeto vivo** → migration é idempotent, roda no SessionStart se DB ausente, lê o que tiver
- **Hooks lentos com DB lock** → WAL mode + writer queue serializa writes; reads concorrentes

## Out of scope

- OpenTelemetry (Phase 2)
- MCP server (Phase 3)
- Embeddings / semantic search (futuro, se chegar a 1000+ docs)

## Concerns

- **knowledge_fts external content mismatch (Wave 1 finding)**: `CREATE VIRTUAL TABLE knowledge_fts USING fts5(name, description, content='knowledge', content_rowid='id')` aceita criação mas `knowledge.id` é TEXT enquanto FTS5 external-content exige INTEGER rowid. Wave 2 (migration) precisa: (a) adicionar `knowledge.row_id INTEGER PRIMARY KEY` separado de `id TEXT UNIQUE`, (b) popular knowledge_fts manualmente sem auto-sync, ou (c) tirar `content='knowledge'` e ter knowledge_fts standalone.

## Checklist

- [x] `src/runtime/event-store.ts` implementado + tipos
- [x] Schema SQL em `src/runtime/schema.sql`
- [x] `src/migrate/jsonl-to-sqlite.ts` idempotent
- [x] Build pipeline: `.ts` → `dist/` consumível por hooks JS (CJS wrapper `templates/hooks/_lib/event-store.js` + re-export em `scripts/_lib/`)
- [x] Hooks migrados: close-gate (read via EventStore), subagent-tracker (write removido); demais hooks já passavam por harness-views (não leem jsonl direto)
- [x] Dashboard lê via EventStore (`templates/scripts/dashboard.js` com fallback legacy)
- [x] Migration testada em sialia (1444 events importados, dashboard --check exit 0)
- [x] Schemas mortos removidos (agentAttempts em scripts + hooks, .subagent-registry write)
- [x] Tests integration: EventStore = buildPipelineState (Wave 5 alignment: 3/3 specs match)
