---
name: commit-workflow
description: Git commit strategy, submodule-aware, budget ≤15 API calls.
disable-model-invocation: true
---
<!-- mustard:generated -->

# Commit Workflow

> Git strategy for mono-repo with submodules. Budget ≤15 API calls.

## Strategy

1. `git status` — see all changes (staged + unstaged)
2. `git diff --stat` — understand scope
3. Group changes by subproject
4. Per subproject: `git add` specific files → `git commit` with conventional message
5. Parent repo: `git add` submodule refs → `git commit`
6. Chain with `&&` — never split into separate commands

## Commit Message Format

```
type(scope): description

- detail 1
- detail 2
```

Types: `feat`, `fix`, `refactor`, `chore`, `docs`, `style`, `test`
Scope: subproject name or module name

## Rules

- Budget: ≤15 API calls total for commit+push
- Combined: commit+push per submodule in 1 wave
- Parent analyzes once, not per submodule
- NEVER use `git add .` or `git add -A` — always specific files
- NEVER commit `.env`, credentials, or large binaries
