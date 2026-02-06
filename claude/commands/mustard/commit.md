# /commit - Simple Commit

> Creates a commit with current changes.

## Usage

```
/commit
/commit "message"
```

## What It Does

1. **Checks** for pending changes
2. **Generates** commit message (if not provided)
3. **Creates** local commit

## Flow

```
/commit
   │
   ▼
git status
   │
   ▼
git diff (staged + unstaged)
   │
   ▼
git log (message style)
   │
   ▼
Generate message
   │
   ▼
git add <files>
   │
   ▼
git commit
```

## Arguments

| Argument | Description | Example |
|----------|-------------|---------|
| `[message]` | Commit message (optional) | `"fix: correct validation"` |

## Examples

```bash
# Commit with auto-generated message
/commit

# Commit with specific message
/commit "feat: add email field to Person"
```

## Message Format

```
<type>: <short description>

<optional body>

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
```

### Types

| Type | Usage |
|------|-------|
| `feat` | New feature |
| `fix` | Bug fix |
| `refactor` | Refactoring |
| `docs` | Documentation |
| `chore` | Maintenance |
| `test` | Tests |

## Output

```
📋 Changes detected:
- M src/features/contract/hooks/useContract.ts
- A src/features/contract/components/ContractForm.tsx

📝 Generated message:
feat: add ContractForm component

Added new form component for contract creation
with validation and error handling.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>

✅ Commit created: abc1234
```

## Notes

- Does **not** push (use `/commit-push` for that)
- Does **not** commit sensitive files (.env, credentials)
- Adds specific files, does not use `git add -A`
- Always includes co-author
