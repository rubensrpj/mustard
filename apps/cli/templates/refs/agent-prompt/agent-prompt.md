# Agent Prompt Template

Orchestrator fills `{placeholders}` before dispatch. Agent receives the rendered version.

Single unified template for all dispatches:
- When `.claude/agents/{subproject}-impl.md` **exists**: orchestrator leaves `{role_block}` empty (role/boundary/validate/return already defined in the custom agent).
- When it **does NOT exist**: orchestrator fills `{role_block}` with `ROLE: {role} — {boundary}` / `Validate: {validate_command}` / `Return: {return_sections}`.

`{context_extras}` is optional (e.g. extra line to read `notes.md`); leave empty when unused.

`{context_md}` is the **relevance-filtered glossary slice** — the subset of `CONTEXT.md` term blocks that match the active spec's entities/files/key-tokens, produced by `context-slice.js`. It sits inside PREFIX-STABLE because the slice is stable for the whole pipeline (the spec does not change mid-run), so the prefix cache holds across dispatches and waves. Leave it empty when `CONTEXT.md` is absent (graceful degrade) — the orchestrator omits the `## SHARED LANGUAGE` block content but keeps the heading harmless. The orchestrator refreshes the slice only on a wave transition.

`{spec_lang}` is filled from the active spec's `### Lang:` header (`pt` or `en`). Orchestrator reads it from `.claude/spec/active/{specName}/spec.md`. Defaults to `en` if missing.

---

## Dispatch Template

> **First-dispatch only.** When `{retry_context}` is non-empty (granular or fix-loop retry), use the **Minimal Retry Template** from `§ Retry Modes` instead — omit CONTEXT, REFERENCE, ENTITY, SKILLS, WEB VALIDATION, ROLE, and RECIPE blocks.

```
<!-- PREFIX-STABLE -->
## CONTEXT
1. Read `{subproject}/CLAUDE.md` — guards, stack, paths
2. Read `{subproject}/.claude/commands/guards.md` — mandatory rules
3. **Sibling-convention check (MANDATORY before first Edit/Write):** for each file you will modify, read ONE neighbouring file in the same directory first to confirm conventions (shebang, license header, async/sync style, error pattern, import order, indentation). Skip only for: NEW directories with no siblings; pure JSON/YAML edits; spec markdown. Cost: ≤1 extra Read per edit target, saves reviewer warnings about "decorative async over blocking", "Bun check after initStore", "import duplicated", etc.
4. Spec language is `{spec_lang}` — applies to spec narrative ONLY (prose, labels, Concerns you append). Source code stays English regardless: identifiers, comments in every form (`//`, `#`, `/* */`, `///`, `'''`, `"""`, doc-comments, `<!-- -->`), file paths, shell commands, AC `Command:` content, log messages. Surgical: never translate pre-existing comments — only write new ones in English.
{context_extras}

## SHARED LANGUAGE
{context_md}

## REFERENCE
{reference_files}

## ENTITY
{entity_info}

## SKILLS
Available skills listed in system. Read SKILL.md only if task matches. Key: {recommended_skills}
Load references/ only for concrete examples.

## WEB VALIDATION
In doubt about API/version/pattern → search web for latest docs before implementing.

## ROLE
{role_block}

## RECIPE
{recipe_context}

## EFFICIENCY
- Absolute paths, no cd
- Read each file once
- Max 3 build attempts, then STOP + report
- Return cap: follow pipeline-config.md Max Return limits (impl 40, explore 30, review 60, plan 80 lines). Focus on: files changed + non-obvious decisions + blockers only.

{retry_context}

<!-- VARIABLE -->
## TASK
{task_steps}

Guards carregados via CLAUDE.md acima — respeite sem exceção.
```

---

## Retry Modes

`{retry_context}` has 3 states:

| Mode | When | `{retry_context}` content |
|------|------|---------------------------|
| `empty` | First dispatch | Empty string — full Dispatch Template above is used |
| `granular` | A step failed (PARTIAL escalation) | Enriched retry header (see below) |
| `fix-loop` | Review returned REJECTED | Enriched retry header with verbatim findings (see below) |

`prior_summary` and `files_modified` come from the latest `.agent-memory/_index.json` entry matching `{agent_type, pipeline}`.

### `granular` format

```
## RETRY CONTEXT
**Mode:** granular
**Prior dispatch:** {prior_summary}
**Files modified previously:**
{files_modified}
**Previous error:** {error_message}
**Resume from step:** {N+1}
```

### `fix-loop` format

```
## RETRY CONTEXT
**Mode:** fix-loop ({K}/2)
**Prior dispatch:** {prior_summary}
**Files modified previously:**
{files_modified}
**Review findings (verbatim):**
{findings_verbatim}
```

### Minimal Retry Template

When `{retry_context}` is non-empty, the orchestrator renders this template instead of the full Dispatch Template. Omits CONTEXT/REFERENCE/ENTITY/SKILLS/WEB VALIDATION/ROLE/RECIPE — prior context is still cached; DON'T re-Read CLAUDE.md/guards/registry unless a modified file changed on disk since last dispatch.

```
<!-- VARIABLE -->
{retry_context}

## EFFICIENCY
- Absolute paths, no cd
- Read each file once (prior context cached — skip CLAUDE.md/guards/registry re-reads unless file changed on disk)
- Max 3 build attempts, then STOP + report
- Return cap: follow pipeline-config.md Max Return limits. Focus on: files changed + non-obvious decisions + blockers only.

## TASK
{task_steps}

Guards carregados via CLAUDE.md acima — respeite sem exceção.
```

---

## Prompt Cache Hit (Anthropic API)

Os marcadores `<!-- PREFIX-STABLE -->` e `<!-- VARIABLE -->` delimitam o que é estável e cacheável entre dispatches. A Anthropic API faz cache automático do prefixo quando dois prompts compartilham os mesmos primeiros ≥1024 tokens byte-identical, cobrando apenas 10% do custo normal nos hits subsequentes. Para o cache realmente ativar, qualquer `{placeholder}` interpolado dentro do bloco PREFIX-STABLE precisa resolver para um valor estável entre dispatches da mesma wave — IDs de skill (não os corpos), nome do role, chave do recipe, paths de subprojeto. Conteúdo dinâmico (spec slice, diff, retry context, task steps) só pode aparecer abaixo do marcador VARIABLE; misturar invalidates o cache inteiro. A Minimal Retry Template não tem prefixo estável (todo conteúdo é dinâmico) — por isso ela é marcada inteira como VARIABLE.

Detalhes da ordem canônica e exemplos: `templates/refs/agent-prompt/prefix-order.md`.

---

## Skill-Based Context Loading

Skills provide progressive disclosure — agents load only what they need:

1. **Metadata** (name + description) — always visible in available skills list (~100 words each)
2. **SKILL.md body** — loaded when agent reads the skill (~500 lines max)
3. **references/** — loaded on-demand when agent needs concrete examples (unlimited)

The orchestrator fills `{recommended_skills}` with skill names most relevant to the task.
Claude natively decides which additional skills to load based on descriptions.

### How to fill `{recommended_skills}`

**Rule 1 — Always prepend `karpathy-guidelines` for code-editing agents.** This includes `impl`, `backend`, `frontend`, `database`, `bugfix` and any agent whose role involves Edit/Write of source code. **Skip** for read-only Explore agents and Review agents (they don't edit, so anti-slop guidelines don't apply).

**Rule 2 — Then list task-relevant skills:**
- Entity/CRUD work → pattern skills for that subproject
- UI/design work → `design-craft` + subproject pattern skills
- Architecture decisions → `senior-architect`
- Complex patterns → relevant advanced pattern skills

**Rule 3 — Append role-based refs to `{context_extras}`:**
- `role=ui` (frontend, mobile) on **feature/enhancement** → append `Read templates/refs/feature/fe-craft-check.md before first Edit/Write — anti-AI-look checklist (DS tokens, states, microinteractions, a11y, content quality)`.
- `role=ui` on **bugfix** → append `Read templates/refs/bugfix/browser-debug.md — Playwright + Chrome DevTools MCP playbook (reproduce → isolate → instrument → fix → prevent)`.
- These refs are stack-agnostic and progressive-disclosure (loaded on demand, not auto-injected). Agents read them once per task.

Examples (replace `{sub}` with actual subproject short name; skill names below are placeholders — pick whatever skills the subproject's `.claude/skills/` actually defines):
- Backend endpoint → `karpathy-guidelines, {sub}-{endpoint-skill}, {sub}-{module-skill}`
- Mobile screen → `karpathy-guidelines, {sub}-{screen-skill}, {sub}-{state-skill}, design-craft` + ref `fe-craft-check.md`
- Frontend section → `karpathy-guidelines, {sub}-{section-skill}, design-craft, react-best-practices` + ref `fe-craft-check.md`
- Frontend bugfix → `karpathy-guidelines, diagnose, {sub}-{relevant-skill}` + ref `browser-debug.md`
- Backend bugfix → `karpathy-guidelines, diagnose, {sub}-{relevant-skill}` (no FE refs)
- Bugfix Explore (diagnostic) → `diagnose` only — **explicit exception to the "Explore gets minimal skills" rule**: the diagnostic loop IS the method of a bug-Explore agent, so `diagnose` belongs there even though Explore agents otherwise skip karpathy and code-editing skills.
- `/task refactor` ASSESS step → `improve-codebase-architecture` alongside any subproject pattern skills; helps the Plan agent surface deepening opportunities before committing to a strategy.
- `/task audit` ASSESS step → `improve-codebase-architecture`; architectural perspective improves severity classification and actionable suggestions.
- Explore (read-only, non-bugfix) → `{sub}-{discovery-skill}` only (no karpathy, no refs)
- Review → review-specific skills only (no karpathy)

ULTRATHINK
