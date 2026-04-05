# Agent Prompt Template

Orchestrator fills `{placeholders}` before dispatch. Agent receives the rendered version.

Single unified template for all dispatches:
- When `.claude/agents/{subproject}-impl.md` **exists**: orchestrator leaves `{role_block}` empty (role/boundary/validate/return already defined in the custom agent).
- When it **does NOT exist**: orchestrator fills `{role_block}` with `ROLE: {role} — {boundary}` / `Validate: {validate_command}` / `Return: {return_sections}`.

`{context_extras}` is optional (e.g. extra line to read `notes.md`); leave empty when unused.

---

## Dispatch Template

```
## CONTEXT
1. Read `{subproject}/CLAUDE.md` — guards, stack, paths
2. Read `{subproject}/.claude/commands/guards.md` — mandatory rules
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

## EFFICIENCY
- Absolute paths, no cd
- Read each file once
- Max 3 build attempts, then STOP + report

{retry_context}

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

Based on task analysis, list the most relevant skill names:
- Entity/CRUD work → pattern skills for that subproject
- UI/design work → `design-craft` + subproject pattern skills
- Architecture decisions → `senior-architect`
- Complex patterns → relevant advanced pattern skills

Examples (replace `{sub}` with actual subproject short name):
- Backend endpoint → `{sub}-endpoint-wiring, {sub}-module-registration`
- Mobile screen → `{sub}-mvvm-feature, {sub}-riverpod-state, design-craft`
- Frontend section → `{sub}-section-component, design-craft, react-best-practices`

ULTRATHINK
