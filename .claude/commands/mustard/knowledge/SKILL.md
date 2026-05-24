---
name: mustard-knowledge
description: Manage the project knowledge base — list entries, search by term, add new patterns/conventions/entities, run memory audit, or generate progress reports. Use when asked about /knowledge, project knowledge, or pipeline-captured patterns.
source: manual
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

Lists all knowledge entries from the `knowledge_patterns` table of the SQLite event store, grouped by type.

### Flow

```bash
rtk mustard-rt run memory list --grouped --format table
```

The binary groups entries by type (pattern / convention / entity) and renders the table. Print the output verbatim. If empty, it reports "No knowledge base found. Run a pipeline or use `/knowledge add` to start one."

---

## glossary

Lists entities from `.claude/entity-registry.json` with their `description` field (extracted from doc-comments by `mustard-rt run sync-registry`).

### Usage

`/knowledge glossary [--filter <term>]`

### Flow

```bash
rtk mustard-rt run knowledge glossary [--filter <term>] --format table
```

The binary reads `.claude/entity-registry.json`, filters (if `--filter` passed), sorts A-Z, and renders `| Entity | Description | Ref |`. Print verbatim. If registry missing/empty: "No entities. Run `/sync-registry` first."

### How descriptions are populated

The description-enricher step in `mustard-rt run sync-registry` reads each entity's first ref file, extracts the immediately-preceding doc-comment block (JSDoc `/** */`, `///`, `//`, `#`), strips markers and `@tag` lines, collapses whitespace, truncates to 200 chars, and sets `entry.description` only when not already set (manual descriptions preserved).

To improve coverage: add doc-comments above entity declarations in your source files.

---

## search

### Usage

`/knowledge search <term>`

### Flow

1. Run `mustard-rt run memory search`
2. Lowercase-match `term` against `name`, `description`, and each tag
3. Display matching entries grouped by type (same format as `list`)
4. If no matches: "No entries matching '{term}' found."

---

## add

Prompts for type/name/description/tags, then pipes JSON to `mustard-rt run memory knowledge`. Confirms: "Knowledge entry '{name}' saved."

---

## notes

Manages persistent project observations injected into agent context during pipelines. These files are **NOT overwritten by `/scan`**.

- No argument: list all notes files and ask which to edit
- `{subproject}`: edit `{subproject}/.claude/commands/notes.md`
- **NEVER** add `<!-- mustard:generated -->` to notes files

---

## audit

Compares auto-memory against CLAUDE.md and skills for duplicates. Outputs a report — **NEVER auto-edits**.

---

## report / evolve / export / import

→ See `../../../refs/knowledge/evolve-report.md`

---

## Rules

- the `knowledge_patterns` table of the SQLite event store is persistent — never deleted by session-cleanup
- `add` and pipeline capture both call the same `mustard-rt run memory knowledge` subcommand
- `search` is case-insensitive
- Always show entry count in list/search output
