# Knowledge Report Reference

> Detail for the `/knowledge report` action: git-based progress reports.
> (The former evolve/export/import actions died with the markdown knowledge
> store — decisions/lessons are per-spec events now; see the /knowledge SKILL.)

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
