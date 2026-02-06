# /status - Status Consolidado

> Shows complete project and workspace status.

## Usage

```
/status
```

## What It Does

1. **Git**: Branch, pending changes, commits ahead/behind
2. **Builds**: Status of each detected project
3. **Tasks**: Tasks in progress
4. **Specs**: Active specs

## Output

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📊 STATUS: {ProjectName}
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

🌿 Git
├── Branch: feature/order
├── Commits: 3 ahead, 0 behind
├── Changes: 2 staged, 1 unstaged
└── Last commit: abc1234 - feat: add order schema

📦 Projects
├── {project_1}/: ✅ Build OK
├── {project_2}/: ✅ Type-check OK
└── {project_3}/: ✅ Tests passed

📋 Tasks
├── #1 [in_progress] Implement Invoice
├── #2 [pending] Create endpoints
└── #3 [pending] Create CRUD

📝 Active Specs
└── spec/active/2026-02-04-invoice/spec.md

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## Sections

### Git

| Info | Description |
|------|-------------|
| Branch | Current branch |
| Commits | Commits ahead/behind remote |
| Changes | Staged and unstaged files |
| Last | Last commit |

### Projects

Shows status of all detected projects (via manifest files).

| Status | Meaning |
|--------|---------|
| ✅ | Build/check passed |
| ❌ | Build/check failed |
| ⏳ | Running |
| ⚠️ | Warnings |

### Tasks

Current TaskList items.

### Specs

Specs in `spec/active/`.

## Notes

- Combines `/where-am-i` and `/workspace-status`
- Fast, does not execute builds (uses cached status)
- Uses cache when available
