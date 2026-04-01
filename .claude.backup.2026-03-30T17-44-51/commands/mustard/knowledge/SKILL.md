# /knowledge - Knowledge Management

> Notes, memory audit, and reports.

## Trigger

`/knowledge <action> [args]`

## Actions

| Action | Description |
|--------|-------------|
| `notes [target]` | Manage project observations |
| `audit` | Audit memory for duplicates |
| `report <period>` | Generate progress report (daily/weekly) |

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
   ═══ MEMORY AUDIT ═══
   Total sections: {n}
   Unique (KEEP): {n}
   Duplicate: {n}

   DUPLICATES FOUND:
   - "{section}" ↔ {source_file}: {similarity}%

   RECOMMENDED PRUNED VERSION:
   {pruned content}
   ═══
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
