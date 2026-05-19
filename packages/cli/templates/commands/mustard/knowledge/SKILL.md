---
name: mustard-knowledge
description: Manage the project knowledge base — list entries, search by term, add new patterns/conventions/entities, run memory audit, or generate progress reports. Use when asked about /knowledge, project knowledge, or pipeline-captured patterns.
---
<!-- mustard:generated -->
# /knowledge - Knowledge Management

> Notes, memory audit, reports, and project knowledge base.

## Trigger

`/knowledge <action> [args]`

## Actions

| Action | Description |
|--------|-------------|
| `list` | List all knowledge entries grouped by type |
| `search <term>` | Search entries by name, description, or tags |
| `glossary` | List entities from `entity-registry.json` with their doc-comment descriptions |
| `add` | Interactively add a knowledge entry |
| `notes [target]` | Manage project observations |
| `audit` | Audit memory for duplicates |
| `report <period>` | Generate progress report (daily/weekly) |
| `evolve` | Analyze clusters and generate recommendations |
| `export` | Export knowledge base to a dated JSON file |
| `import <file>` | Import entries from a shared export file |

---

## list

Reads `.claude/knowledge.json` and displays all entries grouped by type.

### Flow

1. Read `.claude/knowledge.json` (if missing: "No knowledge base found. Run a pipeline or use `/knowledge add` to start one.")
2. Group entries by `type` (pattern / convention / entity)
3. Display formatted:
   ```
   === KNOWLEDGE BASE ({total} entries) ===

   PATTERNS ({n})
   - {name}: {description}  [source: {source}]
     Confidence: {confidence} | Seen: {occurrences}x | Last: {lastSeen}

   CONVENTIONS ({n})
   - {name}: {description}  [source: {source}]
     Confidence: {confidence} | Seen: {occurrences}x | Last: {lastSeen}

   ENTITIES ({n})
   - {name}: {description}  [source: {source}]
     Confidence: {confidence} | Seen: {occurrences}x | Last: {lastSeen}
   ```
   For entries missing `confidence`/`occurrences`/`lastSeen`, display defaults: 0.3 / 1x / (use `updatedAt` or `createdAt`).
4. Show last updated timestamp from newest `updatedAt`

---

## glossary

Lists entities from `.claude/entity-registry.json` along with their `description` field (extracted from doc-comments by `sync-registry.js` post-build via `description-enricher.js`).

### Usage

`/knowledge glossary [--filter <term>]`

### Flow

1. Read `.claude/entity-registry.json` (if missing or empty: "No entities. Run `/sync-registry` first.")
2. Iterate `registry.e` entries (skip keys starting with `_`)
3. For each entity, show:
   ```
   {EntityName}
   {description (or "(no description — add /// or /** ... */ above the declaration in {ref})")}
   ```
4. If `--filter <term>`: case-insensitive match against name OR description
5. Sort by entity name (A-Z)
6. Print total at end: "{N} entities ({M} with descriptions)"

### How descriptions are populated

- `description-enricher.js` runs after `buildRegistry` in `sync-registry.js`
- Reads each entity's first ref file
- Extracts the immediately-preceding doc-comment block (JSDoc `/** */`, triple-slash `///`, line `//`, hash `#`)
- Strips markers, drops `@tag` lines, collapses whitespace, truncates to 200 chars
- Sets `entry.description` only if NOT already set (manual descriptions preserved)

### To improve coverage

Add doc-comments above entity declarations:

```typescript
/** A user account in the platform. */
export class User { ... }

// Tenant-level invoice. Soft-deleted; never hard-purged.
export const invoices = pgTable('invoices', { ... });
```

```csharp
/// <summary>Subscription tier with seat limits.</summary>
public class Plan { ... }
```

```python
# Audit log entry — append-only, never updated.
class AuditEntry: ...
```

---

## search

Filters knowledge entries matching the search term across `name`, `description`, and `tags`.

### Usage

`/knowledge search <term>`

### Flow

1. Read `.claude/knowledge.json`
2. Lowercase-match `term` against `name`, `description`, and each tag
3. Display matching entries grouped by type (same format as `list`)
4. If no matches: "No entries matching '{term}' found."

---

## add

Interactively adds a knowledge entry by prompting the user, then calls `memory.js knowledge`.

### Flow

1. Prompt: "Type? (pattern / convention / entity)"
2. Prompt: "Name?"
3. Prompt: "Description?"
4. Prompt: "Tags? (comma-separated, optional)"
5. Build JSON payload and pipe to script:
   ```bash
   echo '{"type":"...","name":"...","description":"...","source":"manual","tags":[...]}' \
     | bun .claude/scripts/memory.js knowledge
   ```
6. Confirm: "Knowledge entry '{name}' saved."

---

## notes

Manages persistent project observations injected into agent context during pipelines. These files are **NOT overwritten by `/scan`**.

### Targets

| Target | File | Scope |
|--------|------|-------|
| (no argument) | — | Lists all notes files |
| `{subproject}` | `{subproject}/.claude/commands/notes.md` | Subproject agent context |

**Monorepo**: discover targets from `.claude/pipeline-config.md` Agents table or Glob `*/.claude/commands/notes.md`.
**Single repo**: target is root → `.claude/commands/notes.md`.

### Flow — List (`/knowledge notes`)

1. Read each notes file
2. Show summary: which exists, number of observations
3. Ask: "Which notes do you want to edit?"

### Flow — Edit (`/knowledge notes <target>`)

1. Resolve target to file
2. Show current content
3. Ask: "What do you want to add, change, or remove?"
4. Apply edits

### Rules

- **NEVER** add `<!-- mustard:generated -->` to these files
- Keep language consistent
- Observations = concise and actionable

---

## audit

Compares auto-memory against CLAUDE.md and skills to detect duplicated information. Outputs report — **NEVER auto-edits**.

### Flow

1. **Read** user's MEMORY.md
2. **Read** all `{subproject}/CLAUDE.md` + `{subproject}/.claude/commands/guards.md`
3. **Present** report:
   ```
   === MEMORY AUDIT ===
   Total sections: {n}
   Unique (KEEP): {n}
   Duplicate: {n}

   DUPLICATES FOUND:
   - "{section}" <-> {source_file}: {similarity}%

   RECOMMENDED PRUNED VERSION:
   {pruned content}
   ===
   ```
4. **NEVER auto-edit** — user decides

---

## report

Generates daily or weekly progress reports from real git data (log, diff --stat, shortlog). Never invent commits. Categorizes by type and project.

→ See `../../../refs/knowledge/evolve-report.md`

## evolve

Clusters entries by overlapping tags, identifies high-confidence (≥0.7) and emerging (≥3 occurrences) patterns, synthesizes recommendations per cluster.

→ See `../../../refs/knowledge/evolve-report.md`

---

## export

Writes `.claude/knowledge-export-{YYYY-MM-DD}.json` from the full knowledge base.

→ See `../../../refs/knowledge/evolve-report.md`

---

## import <file>

Reads export JSON, pipes each entry to `memory.js knowledge` (deduplication auto-handled). Reports new/updated counts.

→ See `../../../refs/knowledge/evolve-report.md`

---

## Rules

- knowledge.json is persistent — never deleted by session-cleanup
- `add` and pipeline capture both call the same `memory.js knowledge` subcommand
- `search` is case-insensitive
- Always show entry count in list/search output

ULTRATHINK
