'use strict';

/**
 * MCP test helper — spawns the `dist/mcp/mustard-memory.js` server as a Bun
 * child process, performs the `initialize` + `notifications/initialized`
 * handshake, and exposes `listTools()` + `callTool(name, args)`.
 *
 * Protocol: line-delimited JSON-RPC 2.0 over stdio (one JSON object per line).
 * Server stdout = protocol channel; stderr = diagnostics.
 *
 * Fixture seeding: writes a JSON fixture file and invokes
 * `bun tests/integration/mcp-seed.mjs <dbPath> <fixtureFile>` which seeds
 * tables directly via the EventStore SQLite driver. We bypass the
 * JSONL→SQLite migration because its FTS5 external-content path is broken
 * on Windows when knowledge entries are present (pre-existing migration
 * bug, tracked separately). EventStore.knowledge() and the MCP
 * search_knowledge tool both read the base `knowledge` table directly, so
 * skipping knowledge_fts has no impact on this wave's tests.
 */

const { spawn, execFileSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

const REPO_ROOT = path.resolve(__dirname, '..', '..');
const SERVER_PATH = path.join(REPO_ROOT, 'dist', 'mcp', 'mustard-memory.js');
const SEED_PATH = path.join(REPO_ROOT, 'tests', 'integration', 'mcp-seed.mjs');

class McpClient {
  constructor(dbPath) {
    this.proc = spawn('bun', [SERVER_PATH], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, MUSTARD_DB_PATH: dbPath },
    });
    this.id = 0;
    this.buffer = '';
    this.pending = new Map();
    this.stderr = '';
    this.closed = false;

    this.proc.stdout.on('data', (chunk) => {
      this.buffer += chunk.toString('utf8');
      let idx;
      while ((idx = this.buffer.indexOf('\n')) >= 0) {
        const line = this.buffer.slice(0, idx);
        this.buffer = this.buffer.slice(idx + 1);
        if (!line.trim()) continue;
        let msg;
        try {
          msg = JSON.parse(line);
        } catch (_) {
          continue;
        }
        if (msg.id !== undefined && this.pending.has(msg.id)) {
          const resolve = this.pending.get(msg.id);
          this.pending.delete(msg.id);
          resolve(msg);
        }
      }
    });

    this.proc.stderr.on('data', (chunk) => {
      this.stderr += chunk.toString('utf8');
    });

    this.proc.on('exit', () => {
      this.closed = true;
    });
  }

  _send(method, params) {
    return new Promise((resolve, reject) => {
      const id = ++this.id;
      this.pending.set(id, (msg) => {
        if (msg.error) reject(new Error(JSON.stringify(msg.error)));
        else resolve(msg.result);
      });
      const payload = JSON.stringify({ jsonrpc: '2.0', id, method, params });
      this.proc.stdin.write(payload + '\n');
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error('timeout waiting for ' + method + ' (id ' + id + ')' + '\n  server stderr: ' + this.stderr.slice(-500)));
        }
      }, 5000);
    });
  }

  _notify(method, params) {
    // Notifications carry no id and expect no response.
    const payload = JSON.stringify({ jsonrpc: '2.0', method, params });
    this.proc.stdin.write(payload + '\n');
  }

  async initialize() {
    await this._send('initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'mustard-test', version: '0' },
    });
    this._notify('notifications/initialized', {});
  }

  async listTools() {
    return this._send('tools/list', {});
  }

  async callTool(name, args) {
    return this._send('tools/call', { name, arguments: args });
  }

  close() {
    if (!this.closed) this.proc.kill();
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fixture helpers — synthetic project tree under os.tmpdir(), seeded via
// the Bun-backed `mcp-seed.mjs` script (bypasses migration bugs).
// ─────────────────────────────────────────────────────────────────────────────

function makeFixture(label) {
  const rootDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mustard-mcp-' + label + '-'));
  const claudeDir = path.join(rootDir, '.claude');
  const harnessDir = path.join(claudeDir, '.harness');
  fs.mkdirSync(harnessDir, { recursive: true });
  return {
    rootDir,
    claudeDir,
    harnessDir,
    dbPath: path.join(harnessDir, 'mustard.db'),
    fixture: { events: [], knowledge: [], specs: [] },
  };
}

function writeEvents(fixture, events) {
  fixture.fixture.events = events;
}

function writeKnowledge(fixture, knowledge) {
  fixture.fixture.knowledge = knowledge;
}

function writeSpecs(fixture, specs) {
  // Normalize spec shape: helper callers pass { specName, phaseName }; the
  // seeder + EventStore use { name, phase }.
  fixture.fixture.specs = specs.map((s) => ({
    name: s.name ?? s.specName,
    status: s.status ?? 'active',
    phase: s.phase ?? s.phaseName ?? '',
    startedAt: s.startedAt ?? s.createdAt ?? null,
    completedAt: s.completedAt ?? null,
    affectedFiles: s.affectedFiles ?? null,
  }));
}

function runMigration(fixture) {
  // Write fixture file then invoke the seeder under Bun.
  const fixtureFile = path.join(fixture.harnessDir, '_fixture.json');
  fs.writeFileSync(fixtureFile, JSON.stringify(fixture.fixture, null, 2), 'utf8');
  try {
    execFileSync('bun', [SEED_PATH, fixture.dbPath, fixtureFile], {
      cwd: REPO_ROOT,
      stdio: 'pipe',
    });
  } catch (err) {
    const stderr = err.stderr ? err.stderr.toString() : '';
    const stdout = err.stdout ? err.stdout.toString() : '';
    throw new Error('seed failed:\n  stderr: ' + stderr + '\n  stdout: ' + stdout);
  }
  return fixture.dbPath;
}

function cleanup(fixture) {
  try {
    fs.rmSync(fixture.rootDir, { recursive: true, force: true });
  } catch (_) {
    // best-effort tmpdir cleanup
  }
}

module.exports = {
  McpClient,
  makeFixture,
  writeEvents,
  writeKnowledge,
  writeSpecs,
  runMigration,
  cleanup,
};
