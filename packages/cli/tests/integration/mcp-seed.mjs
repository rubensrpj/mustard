#!/usr/bin/env bun
/**
 * MCP test fixture seeder. Bypasses the JSONL→SQLite migration (which has a
 * pre-existing FTS5 external-content bug when knowledge entries are present
 * on Windows) and seeds tables directly via EventStore + raw INSERTs.
 *
 * Invocation:
 *   bun tests/integration/mcp-seed.mjs <dbPath> <jsonFixtureFile>
 *
 * Fixture JSON shape:
 *   {
 *     "events":    [ { ts, sessionId?, wave?, spec?, event, actor?, payload? }, ... ],
 *     "knowledge": [ { id, type, name, description, confidence, createdAt, updatedAt, source }, ... ],
 *     "specs":     [ { name, status, phase, startedAt?, completedAt?, affectedFiles? }, ... ]
 *   }
 *
 * Why ESM (.mjs): top-level `import` from event-store.js (ESM module).
 */

import * as fs from 'node:fs';
import * as path from 'node:path';
import * as url from 'node:url';
import { EventStore } from '../../dist/runtime/event-store.js';

const dbPath = process.argv[2];
const fixturePath = process.argv[3];
if (!dbPath || !fixturePath) {
  console.error('usage: bun mcp-seed.mjs <dbPath> <fixtureJson>');
  process.exit(2);
}

const fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf8'));

const store = new EventStore(dbPath);
store.init();

// EventStore exposes append(event) for events; for knowledge/specs we use the
// runtime-shim driver directly (same constructor EventStore uses internally).
// Concurrent opens are safe under WAL mode.
const { createRequire } = await import('node:module');
const here = url.fileURLToPath(import.meta.url);
const repoRoot = path.resolve(path.dirname(here), '..', '..');
const shimPath = path.join(repoRoot, 'templates', 'hooks', '_lib', 'runtime-shim.js');
const req = createRequire(import.meta.url);
const shim = req(shimPath);
const Ctor = shim.loadSqlite();
if (!Ctor) {
  console.error('SQLite driver unavailable — must run under Bun');
  process.exit(1);
}
const db = new Ctor(dbPath);

// ─ events ─────────────────────────────────────────────────────────────────
for (const ev of fixture.events || []) {
  store.append(ev);
}

// ─ knowledge ──────────────────────────────────────────────────────────────
// Phase 4 Wave 1 fix: knowledge_fts is now standalone (not external-content),
// so seeding it directly is safe and required for MCP search_knowledge to
// return FTS5-ranked results.
if (fixture.knowledge && fixture.knowledge.length > 0) {
  const insertK = db.prepare(
    `INSERT OR REPLACE INTO knowledge
       (id, type, name, description, confidence, created_at, updated_at, source)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)`
  );
  db.exec('BEGIN');
  try {
    for (const k of fixture.knowledge) {
      insertK.run(
        k.id,
        k.type ?? null,
        k.name ?? null,
        k.description ?? null,
        typeof k.confidence === 'number' ? k.confidence : null,
        k.createdAt ?? null,
        k.updatedAt ?? null,
        k.source ?? null
      );
    }
    db.exec('DELETE FROM knowledge_fts');
    db.exec(
      `INSERT INTO knowledge_fts(rowid, id, name, description)
       SELECT ROW_NUMBER() OVER (ORDER BY id), id,
              COALESCE(name, ''), COALESCE(description, '')
       FROM knowledge`
    );
    db.exec('COMMIT');
  } catch (err) {
    db.exec('ROLLBACK');
    throw err;
  }
}

// ─ specs ──────────────────────────────────────────────────────────────────
if (fixture.specs && fixture.specs.length > 0) {
  const insertS = db.prepare(
    `INSERT OR REPLACE INTO specs
       (name, status, phase, started_at, completed_at, affected_files)
     VALUES (?, ?, ?, ?, ?, ?)`
  );
  db.exec('BEGIN');
  try {
    for (const s of fixture.specs) {
      insertS.run(
        s.name,
        s.status ?? 'active',
        s.phase ?? '',
        s.startedAt ?? null,
        s.completedAt ?? null,
        s.affectedFiles ? JSON.stringify(s.affectedFiles) : null
      );
    }
    db.exec('COMMIT');
  } catch (err) {
    db.exec('ROLLBACK');
    throw err;
  }
}

db.close();
store.close();
console.log(JSON.stringify({
  events: (fixture.events || []).length,
  knowledge: (fixture.knowledge || []).length,
  specs: (fixture.specs || []).length,
}));
