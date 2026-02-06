# /report-daily - Daily Report

> Generates daily report of commits and changes.

## Usage

```
/report-daily
/report-daily --date=2026-02-04
```

## What It Does

1. Collects commits from last 24h (or specified date)
2. Groups by author and subproject
3. Lists modified files
4. Calculates statistics

## Expected Output

```markdown
# Daily Report - 2026-02-05

## Commits (12)

### Backend (5 commits)
- bf48162 - refactor: implement SOLID Interface Segregation
- 5ccedc8 - refactor: enforce L8 rule in services
- ...

### Frontend (4 commits)
- ...

### Database (3 commits)
- ...

## Statistics

| Metric | Value |
|--------|-------|
| Total commits | 12 |
| Modified files | 47 |
| Lines added | +892 |
| Lines removed | -234 |

## By Author

| Author | Commits |
|--------|---------|
| user@example.com | 8 |
| claude | 4 |

## Most Modified Files

1. ContractService.cs (5 commits)
2. enforcement.md (3 commits)
3. ...
```

## Options

```
/report-daily --date=YYYY-MM-DD    # Specific date
/report-daily --json               # JSON output
/report-daily --save               # Save to spec/reports/
```

## See Also

- [report-weekly.md](./report-weekly.md) - Weekly report
