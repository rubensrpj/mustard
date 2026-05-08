# Agent Prompt Template

Orchestrator fills `{placeholders}` before dispatch. Agent receives the rendered version.

Single unified template for all dispatches:
- When `.claude/agents/{subproject}-impl.md` **exists**: orchestrator leaves `{role_block}` empty (role/boundary/validate/return already defined in the custom agent).
- When it **does NOT exist**: orchestrator fills `{role_block}` with `ROLE: {role} — {boundary}` / `Validate: {validate_command}` / `Return: {return_sections}`.

`{context_extras}` is optional (e.g. extra line to read `notes.md`); leave empty when unused.

`{spec_lang}` is filled from the active spec's `### Lang:` header (`pt` or `en`). Orchestrator reads it from `.claude/spec/active/{specName}/spec.md`. Defaults to `en` if missing.

---

## Dispatch Template

> **First-dispatch only.** When `{retry_context}` is non-empty (granular or fix-loop retry), use the **Minimal Retry Template** from `§ Retry Modes` instead — omit CONTEXT, REFERENCE, ENTITY, SKILLS, WEB VALIDATION, ROLE, and RECIPE blocks.

```
## CONTEXT
1. Read `{subproject}/CLAUDE.md` — guards, stack, paths
2. Read `{subproject}/.claude/commands/guards.md` — mandatory rules
3. Spec language is `{spec_lang}`. Use `{spec_lang}` for prose, labels, and any Concerns you add. Code/commands stay EN.
{context_extras}

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

Examples (replace `{sub}` with actual subproject short name; skill names below are placeholders — pick whatever skills the subproject's `.claude/skills/` actually defines):
- Backend endpoint → `karpathy-guidelines, {sub}-{endpoint-skill}, {sub}-{module-skill}`
- Mobile screen → `karpathy-guidelines, {sub}-{screen-skill}, {sub}-{state-skill}, design-craft`
- Frontend section → `karpathy-guidelines, {sub}-{section-skill}, design-craft, react-best-practices`
- Bugfix → `karpathy-guidelines, {sub}-{relevant-skill}`
- Explore (read-only) → `{sub}-{discovery-skill}` only (no karpathy)
- Review → review-specific skills only (no karpathy)

ULTRATHINK
