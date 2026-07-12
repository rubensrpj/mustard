# Knowledge Evolve, Report, Export & Import Reference

> Detail for `/knowledge` actions: evolve (cluster analysis), report (git progress), export, and import.

## evolve

Analyzes knowledge entries to find clusters and generate recommendations.

### Procedure

1. Query `mustard-rt run memory list --grouped` to get all knowledge entries
2. Group entries by type, then by overlapping tags
3. Identify **high-confidence patterns** (confidence >= 0.7)
4. Identify **emerging patterns** (occurrences >= 3, confidence < 0.7)
5. Find **clusters**: groups of 3+ entries sharing 2+ tags
6. For each cluster, synthesize a recommendation

### Output Format

```
=== KNOWLEDGE EVOLUTION ===

HIGH CONFIDENCE (confidence >= 0.7):
  - {name}: {description} (seen {occurrences}x, confidence {confidence})

EMERGING (3+ occurrences, confidence < 0.7):
  - {name}: {description} (seen {occurrences}x)

CLUSTERS:
  [{tag1}, {tag2}] — {count} entries
    Recommendation: {synthesized guidance based on entry descriptions}

STATS:
  Total: {n} entries | Patterns: {n} | Conventions: {n} | Entities: {n}
  Avg confidence: {n} | Highest: {name} ({confidence})
===
```

7. If no entries exist, output: "No knowledge entries found. Run pipelines to accumulate patterns."

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

---

## export

Export knowledge base for sharing with team members.

### Procedure

1. Query `mustard-rt run memory list` to get all knowledge entries
2. Generate export file: `.claude/knowledge-export-{YYYY-MM-DD}.json`
3. Write the full knowledge base to the export file
4. Output: "Exported {n} entries to {filepath}"

---

## import <file>

Import knowledge entries from a shared export file.

### Procedure

1. Read the specified import file (JSON format)
2. For each entry in the import:
   - Pipe to `mustard-rt run memory decision` with the entry data
   - Deduplication is handled automatically by the script
3. Report: "Imported: {n} new, {m} updated (duplicates merged with confidence boost)"
