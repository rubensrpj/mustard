# /mtd-git-merge - Merge to Main

> Merges current branch to main/master.

## Usage

```
/mtd-git-merge
```

## What It Does

1. **Checks** for uncommitted changes
2. **Updates** main from remote
3. **Merges** current branch to main
4. **Pushes** main to remote

## Flow

```
/mtd-git-merge
     │
     ▼
git status (check clean)
     │
     ▼
git checkout main
     │
     ▼
git pull origin main
     │
     ▼
git merge <current-branch>
     │
     ▼
git push origin main
     │
     ▼
git checkout <previous-branch>
```

## Prerequisites

- Current branch must be clean (no uncommitted changes)
- Must have push permission on main

## Output

### Success

```
📋 Current branch: feature/invoice
✅ Working tree clean

🔄 Updating main...
✅ main updated

🔀 Merge feature/invoice → main...
✅ Merge completed

🚀 Push main to origin...
✅ Push completed

↩️ Returning to feature/invoice
```

### With Conflicts

```
📋 Current branch: feature/invoice
✅ Working tree clean

🔄 Updating main...
✅ main updated

🔀 Merge feature/invoice → main...
❌ CONFLICTS detected:
- src/mtd-pipeline-features/contract/hooks/useContract.ts

Resolve conflicts and execute:
git add .
git commit
git push origin main
```

## Notes

- Does **not** use force push
- Does **not** delete branch after merge
- Returns to original branch after completion
- Aborts if there are uncommitted changes
