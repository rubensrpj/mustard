# /mtd-report-weekly - Weekly Report

> Generates consolidated weekly activity report.

## Usage

```
/mtd-report-weekly
/mtd-report-weekly --week=2026-W05
```

## What It Does

1. Collects commits from last 7 days (or specified week)
2. Groups by feature/bugfix
3. Calculates productivity metrics
4. Identifies patterns and trends

## Expected Output

```markdown
# Weekly Report - 2026-W05

## Summary

| Metric | Value |
|--------|-------|
| Features implemented | 3 |
| Bugs fixed | 7 |
| Total commits | 45 |
| Lines of code | +2,340 / -890 |

## Implemented Features

### 1. SOLID Interface Segregation
- **Status:** ✅ Completed
- **Commits:** 8
- **Files:** 32
- **Spec:** spec/mtd-pipeline-completed/solid-isp/

### 2. Entity Registry v2.1
- **Status:** ✅ Completed
- **Commits:** 5
- **Files:** 12
- **Spec:** spec/mtd-pipeline-completed/entity-registry/

## Fixed Bugs

| # | Description | Commits |
|---|-------------|---------|
| 1 | Fix L8 violation in PartnerService | 2 |
| 2 | Fix type-check errors in frontend | 3 |
| ... | ... | ... |

## Activity by Day

| Day | Commits | Files |
|-----|---------|-------|
| Mon | 8 | 15 |
| Tue | 12 | 23 |
| Wed | 10 | 18 |
| Thu | 7 | 12 |
| Fri | 8 | 14 |

## By Subproject

| Project | Commits | % |
|---------|---------|---|
| Backend | 20 | 44% |
| Frontend | 15 | 33% |
| Database | 5 | 11% |
| Docs | 5 | 11% |

## Next Week

### In Progress
- [ ] Feature X
- [ ] Feature Y

### Planned
- [ ] Feature Z
```

## Options

```
/mtd-report-weekly --week=YYYY-Www     # Specific week
/mtd-report-weekly --json              # JSON output
/mtd-report-weekly --save              # Save to spec/reports/
/mtd-report-weekly --compare           # Compare with previous week
```

## See Also

- [daily-report.md](./mtd-report-daily.md) - Daily report
