# /commit-push - Commit and Push

## Trigger

`/commit-push`

## Description

Creates commit and pushes to remote.

## Action

1. Same process as /commit
2. Adds `git push` at the end

## Cautions

- Checks if branch has remote configured
- Uses `git push -u origin <branch>` if needed
