# /merge-main - Merge to Main

## Trigger

`/merge-main`

## Description

Merges current branch to main/master.

## Action

1. Checks for uncommitted changes
2. Switches to main branch
3. Pulls latest changes
4. Merges feature branch
5. Pushes to remote

## Cautions

- Will abort if there are merge conflicts
- Requires clean working directory
