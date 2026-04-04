<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Skill Authoring Examples

## Example 1: Simple Foundation Skill (commit-workflow)

```yaml
---
name: commit-workflow
description: Git commit strategy, submodule-aware, budget ≤15 API calls.
disable-model-invocation: true
---
<!-- mustard:generated -->

# Commit Workflow

> Git strategy for mono-repo with submodules. Budget ≤15 API calls.

## Strategy
1. `git status` — see all changes
2. `git diff --stat` — understand scope
3. Group changes by subproject
4. Per subproject: `git add` specific files → `git commit`

## Rules
- Budget: ≤15 API calls total
- NEVER use `git add .` — always specific files
```
Ref: `skills/commit-workflow/SKILL.md`

## Example 2: Complex Skill with References (pipeline-execution)

```yaml
---
name: pipeline-execution
description: Pipeline phases, dispatch rules, wave system, validate, retry.
  Load for /feature /resume /approve.
disable-model-invocation: true
---
<!-- mustard:generated -->

# Pipeline Execution Detail

> Phases, role rules, dispatch mechanics, validation, bugfix paths.

## Pipeline Feature
### ANALYZE Phase
1. AUTO-SYNC: `node .claude/scripts/sync-registry.js`
2. Read `entity-registry.json` → entity found? → infer layers
```
Ref: `skills/pipeline-execution/SKILL.md`

## Example 3: Skill with Rich References (design-craft)

Structure with multiple reference files:

```
skills/design-craft/
├── SKILL.md
└── references/
    ├── critique.md
    ├── example.md
    ├── palettes-catalog.md
    ├── principles.md
    ├── styles-catalog.md
    ├── typography-catalog.md
    ├── ux-guidelines.md
    └── validation.md
```
Ref: `skills/design-craft/SKILL.md`
