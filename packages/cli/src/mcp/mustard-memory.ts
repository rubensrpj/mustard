/**
 * Mustard Memory — MCP server exposing EventStore + KnowledgeBase as tools.
 *
 * Read-only by design. Writes happen in hooks (PreToolUse/PostToolUse) where
 * sessionId/wave/spec attribution is authentic. MCP exposes queries only.
 *
 * Phase 3 — Wave 1: server core + 5 tools (search_knowledge, query_events,
 * find_similar_specs, get_spec_metrics, get_span_summary).
 *
 * Runtime: requires Bun (EventStore depends on bun:sqlite). Spawned by Claude
 * Code via `settings.json` mcpServers config: { command: "bun", args: [...] }.
 *
 * IPC: stdio. NEVER write logs to stdout — that channel is reserved for the
 * MCP JSON-RPC protocol. Diagnostics go to stderr.
 */

import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import * as path from 'node:path';
import { EventStore } from '../runtime/event-store.js';

// ---------------------------------------------------------------------------
// DB path resolution
// ---------------------------------------------------------------------------

function resolveDbPath(): string {
  const envPath = process.env.MUSTARD_DB_PATH;
  if (envPath) return path.resolve(envPath);
  return path.resolve(process.cwd(), '.claude/.harness/mustard.db');
}

// ---------------------------------------------------------------------------
// EventStore init — fail fast if Bun missing. Server can't run without SQLite.
// settings.json pins `command: "bun"` so this is a hard precondition.
// ---------------------------------------------------------------------------

const store = new EventStore(resolveDbPath());
try {
  store.init();
} catch (err) {
  // Log to stderr (stdout is MCP protocol channel).
  const msg = err instanceof Error ? err.message : String(err);
  process.stderr.write(`[mustard-memory] EventStore.init failed: ${msg}\n`);
  process.exit(1);
}

// ---------------------------------------------------------------------------
// Server + tool registration
// ---------------------------------------------------------------------------

const server = new McpServer({ name: 'mustard-memory', version: '2.0.0' });

function jsonResult(data: unknown) {
  return { content: [{ type: 'text' as const, text: JSON.stringify(data, null, 2) }] };
}

// Tool 1: search_knowledge
// Uses EventStore.knowledge({search}) → knowledge_fts MATCH with bm25 ordering.
// Type filter is post-FTS in-process (small result set after MATCH); preserves
// the same input schema as before so consumers don't break.
server.registerTool(
  'search_knowledge',
  {
    description:
      'Full-text search past learnings/decisions/patterns from the EventStore knowledge table',
    inputSchema: {
      query: z.string().min(1),
      type: z.enum(['pattern', 'convention', 'entity']).optional(),
      limit: z.number().int().min(1).max(50).default(10),
    },
  },
  async ({ query, type, limit }) => {
    // Over-fetch slightly so the type post-filter still returns `limit` rows
    // in the typical case. Cheap because bm25 ordering caps work to MATCH hits.
    const candidates = store.knowledge({ search: query, limit: limit * 5 });
    const filtered = type
      ? candidates.filter((k) => k.type === type)
      : candidates;
    return jsonResult(filtered.slice(0, limit));
  }
);

// Tool 2: query_events
server.registerTool(
  'query_events',
  {
    description: 'Filter events by spec/event/since (ISO ts). Returns up to `limit` rows.',
    inputSchema: {
      spec: z.string().optional(),
      event: z.string().optional(),
      since: z.string().optional(),
      limit: z.number().int().min(1).max(500).default(100),
    },
  },
  async ({ spec, event, since, limit }) => {
    const filter: { spec?: string; event?: string; since?: string } = {};
    if (spec !== undefined) filter.spec = spec;
    if (event !== undefined) filter.event = event;
    if (since !== undefined) filter.since = since;
    const rows = store.query(filter).slice(0, limit);
    return jsonResult(rows);
  }
);

// Tool 3: find_similar_specs
// EventStore.specs() returns the full projection (no FTS yet). Score by token
// overlap on name + phase + affectedFiles. Phase 4 may add a specs_fts table;
// the tool signature is stable across that swap.
server.registerTool(
  'find_similar_specs',
  {
    description:
      'Rank specs by token overlap against a free-text description (name + phase + affectedFiles)',
    inputSchema: {
      description: z.string().min(1),
      limit: z.number().int().min(1).max(20).default(5),
    },
  },
  async ({ description, limit }) => {
    const tokens = description
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean);
    if (tokens.length === 0) return jsonResult([]);
    const all = store.specs();
    const matches = all
      .map((s) => {
        const hay = `${s.name} ${s.phase ?? ''} ${(s.affectedFiles ?? []).join(' ')}`.toLowerCase();
        const score = tokens.reduce((acc, t) => acc + (hay.includes(t) ? 1 : 0), 0);
        return { spec: s, score };
      })
      .filter((m) => m.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, limit);
    return jsonResult(matches);
  }
);

// Tool 4: get_spec_metrics
server.registerTool(
  'get_spec_metrics',
  {
    description: 'Return the metrics_projection row for a spec, or { error } if missing',
    inputSchema: { spec: z.string().min(1) },
  },
  async ({ spec }) => {
    const m = store.metrics(spec);
    return jsonResult(m ?? { error: 'no metrics for spec', spec });
  }
);

// Tool 5: get_span_summary
// Aggregates token/duration across spans filtered by spec/phase. Returns
// totals plus a per-model breakdown — useful for `/mustard:stats`-style views
// over MCP without re-implementing the aggregator client-side.
server.registerTool(
  'get_span_summary',
  {
    description:
      'Aggregated token/duration summary from the spans table; groups by model',
    inputSchema: {
      spec: z.string().optional(),
      phase: z.string().optional(),
      limit: z.number().int().min(1).max(5000).default(1000),
    },
  },
  async ({ spec, phase, limit }) => {
    const filter: { spec?: string; phase?: string; limit: number } = { limit };
    if (spec !== undefined) filter.spec = spec;
    if (phase !== undefined) filter.phase = phase;
    const spans = store.spans(filter);
    const byModel: Record<string, { count: number; in: number; out: number; durationMs: number }> = {};
    let totalInputTokens = 0;
    let totalOutputTokens = 0;
    let totalDurationMs = 0;
    for (const s of spans) {
      totalInputTokens += s.inputTokens || 0;
      totalOutputTokens += s.outputTokens || 0;
      totalDurationMs += s.durationMs || 0;
      const model = s.model || 'unknown';
      const bucket = byModel[model] ?? { count: 0, in: 0, out: 0, durationMs: 0 };
      bucket.count += 1;
      bucket.in += s.inputTokens || 0;
      bucket.out += s.outputTokens || 0;
      bucket.durationMs += s.durationMs || 0;
      byModel[model] = bucket;
    }
    return jsonResult({
      count: spans.length,
      totalInputTokens,
      totalOutputTokens,
      totalDurationMs,
      byModel,
    });
  }
);

// ---------------------------------------------------------------------------
// Connect stdio transport. Top-level await is fine — tsconfig targets ES2022
// and module=NodeNext; output is ESM.
// ---------------------------------------------------------------------------

const transport = new StdioServerTransport();
await server.connect(transport);
