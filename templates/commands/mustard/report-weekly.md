# /report-weekly - Weekly Report

## Trigger

`/report-weekly`

## Description

Generates a weekly progress report.

## Data Collection

```bash
git log --oneline --since="1 week ago"
git diff --stat @{1.week.ago}
git shortlog -sn --since="1 week ago"
```

## Output Template

```markdown
# Weekly Report: {YYYY-Wnn}

## Period
{start date} to {end date}

## Executive Summary
{Paragraph summarizing the week}

## Metrics

| Metric | Value |
| ------ | ----- |
| Commits | {n} |
| Features | {n} |
| Bugfixes | {n} |
| Lines added | {n} |
| Lines removed | {n} |

## Implemented Features
1. **{Feature 1}**: {description}

## Bugs Fixed
1. **{Bug 1}**: {description}

## Changes by Project

### Backend
- {change 1}

### Frontend
- {change 1}

### Database
- {change 1}

## Next Week
- {Planning if available}
```

## Rules

- Use real git data only — do not invent commits
- Categorize by type and project
- Save to `reports/weekly/{date}.md`
