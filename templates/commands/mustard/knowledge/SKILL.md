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
| `add` | Interactively add a knowledge entry |
| `notes [target]` | Manage project observations |
| `audit` | Audit memory for duplicates |
| `report <period>` | Generate progress report (daily/weekly) |

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

   CONVENTIONS ({n})
   - {name}: {description}  [source: {source}]

   ENTITIES ({n})
   - {name}: {description}  [source: {source}]
   ```
4. Show last updated timestamp from newest `updatedAt`

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

Interactively adds a knowledge entry by prompting the user, then calls `knowledge-update.js`.

### Flow

1. Prompt: "Type? (pattern / convention / entity)"
2. Prompt: "Name?"
3. Prompt: "Description?"
4. Prompt: "Tags? (comma-separated, optional)"
5. Build JSON payload and pipe to script:
   ```bash
   echo '{"type":"...","name":"...","description":"...","source":"manual","tags":[...]}' \
     | node .claude/scripts/knowledge-update.js
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

**Monorepo**: discover targets from `pipeline-config.md` Agents table or Glob `*/.claude/commands/notes.md`.
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

Generates progress reports from git data.

### Usage

`/knowledge report daily` or `/knowledge report weekly`

### Daily Report

```bash
git log --oneline --since="00:00" --until="23:59"
git diff --stat HEAD~10
```

Output: Summary, commits by type (feat/fix/chore), modified files by project, highlights, pending items.

### Weekly Report

```bash
git log --oneline --since="1 week ago"
git diff --stat @{1.week.ago}
git shortlog -sn --since="1 week ago"
```

Output: Executive summary, metrics table, implemented features, bugs fixed, changes by project, next week planning.

### Rules

- Use real git data only — do not invent commits
- Categorize commits by type and project

## Rules

- knowledge.json is persistent — never deleted by session-cleanup
- `add` and pipeline capture both call the same `knowledge-update.js` script
- `search` is case-insensitive
- Always show entry count in list/search output

ULTRATHINK
