# /commit-push - Commit and Push

> Creates commit and sends to remote repository.

## Usage

```
/commit-push
/commit-push "message"
```

## What It Does

1. **Checks** for pending changes
2. **Generates** commit message (if not provided)
3. **Creates** local commit
4. **Pushes** to remote

## Flow

```
/commit-push
     │
     ▼
  /commit
     │
     ▼
  git push
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `[message]` | Commit message (optional) | `"fix: correct validation"` |

## Examples

```bash
# Commit and push with auto-generated message
/commit-push

# With specific message
/commit-push "feat: add email field"
```

## Output

```
📋 Changes detected:
- M src/features/contract/hooks/useContract.ts

📝 Generated message:
fix: update contract validation

✅ Commit created: abc1234
🚀 Push to origin/dev... OK
```

## Notes

- Executes `/commit` first
- Pushes to current branch
- Uses `-u` if branch has no upstream
