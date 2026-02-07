# /report-daily - Daily Report

## Trigger

`/report-daily`

## Description

Generates a daily progress report.

## Data Collection

```bash
git log --oneline --since="00:00" --until="23:59"
git diff --stat HEAD~10
```

## Output Template

```markdown
# Daily Report: {YYYY-MM-DD}

## Summary
{Paragraph summarizing the day}

## Commits ({total})

### Feature
- {hash} {message}

### Bugfix
- {hash} {message}

### Chore
- {hash} {message}

## Modified Files
| Project | Files | Lines +/- |
| ------- | ----- | --------- |
| Backend | {n} | +{a}/-{r} |
| Frontend | {n} | +{a}/-{r} |
| Database | {n} | +{a}/-{r} |

## Highlights
- {Highlight 1}

## Pending
- {If any}
```

## Rules

- Use real git data only — do not invent commits
- Categorize commits by type (feat, fix, chore)
- Save to `reports/daily/{date}.md`
