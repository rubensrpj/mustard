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

`mustard-rt run scan-orchestrate` generates these **deterministically** (no LLM) — this section documents the contract, not a manual step. One `.claude/agents/{subproject.name}-impl.md` + `{subproject.name}-explorer.md` per detected subproject (named by subproject name, NOT role), written to the **root** `.claude/agents/` catalog:
- YAML frontmatter: name (`{subproject.name}-impl`), **routing-grade description** (derived from subproject name + stack + role + discovered cluster labels — e.g. "Implementation agent for the api subproject (Rust, backend). Use when editing or building code under api/. Owns these conventions: Service, Repository." — NOT a generic "role implementation for X"), model (sonnet default), tools, memory (project)
- Body: a `> trust these facts` lead, mandatory reads, the subproject's **Guards** (extracted from `{path}/CLAUDE.md`), **Recommended Skills** (resolved deterministically via `skill-resolve` for the role+subproject), **Pre-mined clusters** table (from the entity-registry `_patterns`), boundary, validation, return format. The explorer variant omits the writable Guards block but keeps clusters + skills, and is read-only (`tools: [Read, Grep, Glob]`).
- Mark `<!-- mustard:generated at:{ISO} role:{role} -->` AFTER closing `---` (NEVER before opening `---` — breaks YAML frontmatter parsing)
- **Idempotent:** a non-force scan only writes a missing agent; `--force` regenerates a `mustard:generated` agent but **preserves a hand-authored one** (no generated marker).

### Dispatch via native `subagent_type`

Because the rich agent carries guards/skills/clusters in its own system prompt, EXECUTE/explore dispatch should pass `subagent_type: "{subproject.name}-impl"` (or `-explorer`) when the file exists — Claude Code applies that prompt natively, so the parent does not re-send the same context (token economy). When no rich agent exists (first scan, or a preserved manual agent without one), fall back to `subagent_type: "general-purpose"` with the full rendered prompt. `agent-prompt-render` mirrors this: it suppresses `{role_block}` when the rich agent is present (see `refs/agent-prompt/agent-prompt.md`).

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
