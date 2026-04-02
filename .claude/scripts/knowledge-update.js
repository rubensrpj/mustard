#!/usr/bin/env node
/**
 * KNOWLEDGE-UPDATE: Append knowledge entries to the project knowledge base
 *
 * Input schema (stdin):
 *   {
 *     "type": "pattern" | "convention" | "entity",
 *     "name": "...",
 *     "description": "...",
 *     "source": "pipeline-name or agent-type",
 *     "tags": ["optional", "tags"],
 *     "cwd": "/optional/project/root"
 *   }
 *
 * Behavior:
 *   1. Parse JSON from stdin
 *   2. Read existing knowledge.json or create new
 *   3. Check for duplicates (same name + type)
 *   4. If duplicate: update description and timestamp
 *   5. If new: append entry
 *   6. Prune if exceeds 200 entries total (oldest per category)
 *   7. Write back
 *   8. Exit 0 always (fail-open)
 *
 * @version 1.1.0
 */

const fs = require('fs');
const path = require('path');

const MAX_ENTRIES = 200;
const MAX_PER_CATEGORY = 80;

async function main() {
  let raw = '';
  for await (const chunk of process.stdin) {
    raw += chunk;
  }

  let input;
  try {
    input = JSON.parse(raw);
  } catch (err) {
    process.stderr.write(`[knowledge-update] Failed to parse stdin: ${err.message}\n`);
    process.exit(0);
  }

  try {
    const cwd = input.cwd || process.cwd();
    const kbPath = path.join(cwd, '.claude', 'knowledge.json');

    // Read or create knowledge base
    let kb = { version: 1, entries: [] };
    try {
      if (fs.existsSync(kbPath)) {
        kb = JSON.parse(fs.readFileSync(kbPath, 'utf8'));
        if (!kb.entries) kb.entries = [];
      }
    } catch {
      kb = { version: 1, entries: [] };
    }

    const type = String(input.type || 'pattern');
    const name = String(input.name || '').trim();
    const description = String(input.description || '').trim();
    const source = String(input.source || 'unknown');
    const tags = Array.isArray(input.tags) ? input.tags : [];

    if (!name || !description) {
      process.stderr.write('[knowledge-update] Missing name or description\n');
      process.exit(0);
    }

    // Check for duplicate (same name + type)
    const existingIdx = kb.entries.findIndex(
      e => e.name === name && e.type === type
    );

    const timestamp = new Date().toISOString();

    if (existingIdx >= 0) {
      // Update existing — boost confidence and occurrence count
      const existing = kb.entries[existingIdx];
      existing.description = description;
      existing.source = source;
      existing.tags = tags;
      existing.updatedAt = timestamp;

      // Backwards compatibility: add fields on first update if missing
      const prevOccurrences = existing.occurrences != null ? existing.occurrences : 1;
      existing.occurrences = prevOccurrences + 1;
      existing.confidence = Math.min(1.0, 0.3 + (existing.occurrences * 0.1));
      existing.lastSeen = timestamp;
    } else {
      // Add new
      kb.entries.push({
        id: `${type}-${Date.now()}`,
        type,
        name,
        description,
        source,
        tags,
        confidence: 0.3,
        occurrences: 1,
        createdAt: timestamp,
        updatedAt: timestamp,
        lastSeen: timestamp,
      });
    }

    // Prune: per category, keep newest MAX_PER_CATEGORY
    const byType = {};
    for (const e of kb.entries) {
      if (!byType[e.type]) byType[e.type] = [];
      byType[e.type].push(e);
    }

    const pruned = [];
    for (const [, entries] of Object.entries(byType)) {
      entries.sort((a, b) => new Date(b.updatedAt || b.createdAt) - new Date(a.updatedAt || a.createdAt));
      pruned.push(...entries.slice(0, MAX_PER_CATEGORY));
    }

    // Global cap
    pruned.sort((a, b) => new Date(b.updatedAt || b.createdAt) - new Date(a.updatedAt || a.createdAt));
    kb.entries = pruned.slice(0, MAX_ENTRIES);

    // Ensure directory exists
    const dir = path.dirname(kbPath);
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }

    fs.writeFileSync(kbPath, JSON.stringify(kb, null, 2), 'utf8');

  } catch (err) {
    process.stderr.write(`[knowledge-update] Error: ${err.message}\n`);
  }

  process.exit(0);
}

main();
