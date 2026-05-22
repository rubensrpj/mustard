# Agent Generation & Implementation Recipes Reference

> Detail for `/scan` §8 (Implementation Recipes) and §9 (Agent Generation).

## 8. Implementation Recipes

Generate `recipes.md` — implementation index for the orchestrator.

Analyze import chains, module structure, and file dependencies to discover the implementation hierarchy. Generate recipes for recurring implementation types.

### Analysis: 1) Import chains → dependency levels 2) Extract sequences from `patterns*.md` 3) Build recipe per type (new entity, modify, new endpoint) 4) Pick reference modules per complexity 5) Split at natural dependency boundaries

### Format per recipe:
```markdown
## Recipe: {Type Name}
### Steps
1. {Step} → `{pattern-file}.md` §{N}
N. Build/type-check
### Reference module: {Name} | Reference files: {path/to/file1}, {path/to/file2}
### Task splits
- **{SplitName}** (steps {range}): Patterns: `{file}.md` | Depends on: {none | split | "agent:{role} completed"}
### File hierarchy
| Level | Component | Depends on |
|-------|-----------|-----------|
| 1 | {base} | — |
| N | build check | all |
```

Orchestrator uses hierarchy for: implementation ORDER, task SPLIT boundaries, CROSS-AGENT deps. Rules: exact §N refs | 3-5 reference paths | max ~10 files/split | ends with build check | hierarchy from actual imports

## 9. Agent Generation

Generate `.claude/agents/{subproject.name}-impl.md` per detected subproject (named by subproject name, NOT role):
- YAML frontmatter: name (`{subproject.name}-impl`), description, model (sonnet default), tools, memory (project)
- Body: mandatory reads, boundary, validation, return format
- Mark `<!-- mustard:generated -->` AFTER closing `---` (NEVER before opening `---` — breaks YAML frontmatter parsing)
- Explorer agent always generated (`{subproject.name}-explorer.md`, model: sonnet, read-only tools, with skill refs — see `scan.md` §4.5 for full template)

| Role | Tools | Boundary |
|------|-------|----------|
| api | Read,Write,Edit,Bash,Grep,Glob | Server-side only |
| ui | Read,Write,Edit,Bash,Grep,Glob | UI + types only |
| library | Read,Write,Edit,Bash,Grep,Glob | Same as api |

Validation command: extracted from subproject CLAUDE.md → Commands section (build/type-check).

### Agent Location

Agents are generated ONLY in root `.claude/agents/` (NOT in subproject `.claude/agents/`).
The orchestrator dispatches agents from root level.

### Cleanup Stale Subproject Files

When regenerating subproject agents/skills:
- Delete files referencing wrong project names (e.g., leftovers from previous projects that don't match current subproject names)
- Clean up empty `.agent-state/` and `spec/` directories if they contain no user content
- Preserve `agent-memory/` — may contain useful investigation notes from past sessions
