# Mustard 2.0 — Phase 3: MCP Memory Server

- **Lang**: ptbr
- **Status**: completed
- **Phase**: CLOSE
- **Checkpoint**: 2026-05-12T21:00:00Z
- **Scope**: Full
- **Type**: feature
- **Model**: opus
- **Depends on**: Phase 1 (EventStore), Phase 2 (Telemetry)
- **Unlocks**: smarter orchestrator (consults past learnings via MCP). Dashboard standalone agora é projeto separado (spec `mustard-dashboard-1-0-standalone-tauri`).

## Summary

Servir EventStore + KnowledgeBase via MCP server (`@modelcontextprotocol/sdk`). Claude Code consulta memória do Mustard como tool durante o turno — sem re-implementar leitura, sem duplicar lógica, sem inflar contexto. Outros agentes (Cursor, Aider, qualquer Claude Code project) podem consultar mesmo store.

**NOTA**: Dashboard.js refactor REMOVIDO desta fase — dashboard vira produto Tauri standalone (spec separada). Mustard core deprecia `templates/scripts/dashboard*.js` em release 2.x.

## Problem

Hoje:
- **Cada hook** que lê knowledge.json re-implementa parse + ranking
- **Dashboard** re-implementa agregação (paralela ao buildPipelineState)
- **Outros agentes** não conseguem consultar memória do Mustard — só hooks rodando no projeto
- **Orchestrator não consulta knowledge durante o turno** — só recebe injection no SessionStart (top-5 capped 500 tokens)

Com Phase 1 isso vira O(log n) com FTS5, mas o consumidor precisa interface estável. MCP é o padrão estabelecido.

## Goal

`mustard-memory` MCP server rodando localmente expõe tools:

- `search_knowledge(query, type?, limit?)` — FTS5 sobre knowledge
- `query_events(spec?, event?, since?, limit?)` — replay parcial
- `find_similar_specs(description)` — FTS5 sobre specs.summary
- `get_spec_metrics(spec)` — projection from metrics_projection table
- `get_span_summary(filter)` — tokens/duration por fase/modelo/agent

Registrado em `settings.json` como MCP server. Claude Code chama via `mcp__mustard_memory__search_knowledge(...)` durante orchestration.

Dashboard reescrito como **cliente MCP** — zero duplicação de lógica de query.

## Acceptance Criteria

1. **MCP server inicia e expõe tools**
   ```bash
   timeout 5 node dist/mcp/mustard-memory.js < /dev/null > /tmp/mcp-init.txt 2>&1; grep -q "tools" /tmp/mcp-init.txt
   ```
   Server printa lista de tools no stdio MCP handshake.

2. **search_knowledge retorna FTS5-ranked**
   ```bash
   node tests/integration/mcp-search-knowledge.js
   ```
   Query "auth" em base de teste retorna entries ordenadas por relevância (FTS5 bm25 score).

3. **query_events filter funciona**
   ```bash
   node tests/integration/mcp-query-events.js
   ```
   Filter `{spec:'telegram-alerting', event:'tool.use'}` retorna só eventos taggeados pra essa spec.

4. **find_similar_specs**
   ```bash
   node tests/integration/mcp-similar-specs.js
   ```
   Query "user authentication flow" em base com `auth-roadmap` spec retorna ≥1 match com score >0.

5. **MCP registrado em settings.json**
   ```bash
   node -e "const j=require('./templates/settings.json');const m=j.mcpServers&&j.mcpServers['mustard-memory'];process.exit(m&&m.command?0:1)"
   ```
   `templates/settings.json` tem `mcpServers.mustard-memory` configurado.

6. **Dashboard scripts deprecated com aviso**
   ```bash
   grep -q "DEPRECATED" templates/scripts/dashboard.js && grep -q "mustard-dashboard" templates/scripts/dashboard.js
   ```
   `templates/scripts/dashboard.js` ganha banner de deprecation apontando pra produto Tauri standalone. Funcionalidade mantida 1 release pra compat.

7. **Performance: query <10ms via MCP local**
   ```bash
   node tests/integration/mcp-latency.js
   ```
   100 queries `search_knowledge` em loop: p95 <10ms (overhead MCP + FTS5).

8. **Auth & sandbox**
   ```bash
   node tests/integration/mcp-sandbox.js
   ```
   MCP server **read-only** — tentativa de `append_event` retorna error (write requer hook).

### Parseable AC (cross-shell, QA-runner)

Tests usam extensão `.cjs` (project tem `"type": "module"`).

- [ ] AC-2: search_knowledge tool returns ranked results — Command: `bun test tests/integration/mcp-search-knowledge.cjs`
- [ ] AC-3: query_events filter works — Command: `bun test tests/integration/mcp-query-events.cjs`
- [ ] AC-4: find_similar_specs returns matches — Command: `bun test tests/integration/mcp-similar-specs.cjs`
- [ ] AC-5: settings.json has mcpServers.mustard-memory — Command: `node -e "const j=require('./templates/settings.json');const m=j.mcpServers&&j.mcpServers['mustard-memory'];process.exit(m&&m.command?0:1)"`
- [ ] AC-6: dashboard files have deprecation banner — Command: `node -e "const fs=require('fs');for(const f of ['templates/scripts/dashboard.js','templates/scripts/dashboard-ui.js']){const c=fs.readFileSync(f,'utf8');if(!c.includes('DEPRECATED')||!c.includes('mustard-dashboard')){console.log('FAIL',f);process.exit(1)}}process.exit(0)"`
- [ ] AC-7: MCP latency p95 < 10ms — Command: `bun test tests/integration/mcp-latency.cjs`
- [ ] AC-8: MCP read-only sandbox — Command: `bun test tests/integration/mcp-sandbox.cjs`

## Implementation

### MCP server (TypeScript + Bun)

```typescript
// src/mcp/mustard-memory.ts
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { EventStore } from '../runtime/event-store.js';

const server = new McpServer({ name: 'mustard-memory', version: '2.0.0' });
const store = new EventStore(resolveDbPath());

server.tool('search_knowledge', {
  description: 'Full-text search past learnings/decisions/patterns',
  inputSchema: { query: z.string(), type: z.enum(['pattern','convention','entity']).optional(), limit: z.number().min(1).max(50).default(10) }
}, async ({ query, type, limit }) => {
  const results = store.knowledge({ search: query, type, limit });
  return { content: [{ type: 'text', text: JSON.stringify(results, null, 2) }] };
});

server.tool('query_events', {/* ... */});
server.tool('find_similar_specs', {/* ... */});
server.tool('get_spec_metrics', {/* ... */});
server.tool('get_span_summary', {/* ... */});

await server.connect(new StdioServerTransport());
```

### settings.json wiring

```json
{
  "mcpServers": {
    "mustard-memory": {
      "command": "bun",
      "args": [".claude/dist/mcp/mustard-memory.js"],
      "env": { "MUSTARD_DB_PATH": ".claude/.harness/mustard.db" }
    }
  }
}
```

### Dashboard scripts: deprecation path

`templates/scripts/dashboard*.js` recebem banner de deprecation no topo:

```javascript
/**
 * @deprecated Mustard 2.x — Dashboard local em JS será removido na 3.0.
 * Substituído pelo produto standalone "Mustard Dashboard" (Tauri desktop app).
 * Veja: https://mustard-dashboard.dev (ou docs/dashboard-migration.md)
 */
```

Funcionalidade mantida 1 release pra usuários migrarem. Mustard 3.0 remove ~80KB de código.

### Read-only safety

MCP server **só lê**. Writes acontecem em hooks (PreToolUse/PostToolUse). Razão:
- Hooks têm contexto autêntico (sessionId, wave, spec)
- MCP é consumido por agentes — write via agente abriria injection attacks
- Phase 4 hardening adiciona schema validation se quisermos abrir write controlado

### Auto-start

MCP server registrado em `settings.json` → Claude Code spawna no SessionStart. Zero ação do usuário.

## Risks

- **MCP overhead**: stdio IPC + JSON serialization. Mitigação: medido em AC #7 (p95 <10ms).
- **DB lock entre hook write e MCP read**: WAL mode (Phase 1) resolve — readers concorrem com writer.
- **MCP SDK breakage entre versões**: pinned em `package.json`; CI roda teste de integração contra release atual.

## Out of scope

- MCP write tools (read-only nesta fase)
- Remote MCP (só local nesta fase)
- Vector search semantic (Phase futura se escalar)
- Auth multi-projeto (single-project DB nesta fase)

## Checklist

- [x] `@modelcontextprotocol/sdk` 1.29.0 + zod 4.4.3 instalados (deps runtime)
- [x] `src/mcp/mustard-memory.ts` com 5 tools (search_knowledge, query_events, find_similar_specs, get_spec_metrics, get_span_summary) via registerTool API
- [x] `templates/settings.json` mcpServers registered
- [x] Dashboard.js banner @deprecated (refactor pra MCP client adiado — vira Tauri standalone)
- [x] Tests: 5 MCP integration + latency (p95 2.05ms) + sandbox (read-only verified)
- [x] Doc: `docs/mcp-tools.md` com exemplo de uso por agente
