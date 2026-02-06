# /commit - Simple Commit

## Trigger

`/commit`

## Description

Creates a commit with current changes.

## Action

1. Runs `git status` to see changes
2. Runs `git diff` to analyze content
3. Generates commit message based on changes
4. Runs `git add` + `git commit`

## Message Format

```
<type>: <short description>

<detailed description if needed>

Co-Authored-By: Claude <noreply@anthropic.com>
```

Types: feat, fix, refactor, docs, chore, test
