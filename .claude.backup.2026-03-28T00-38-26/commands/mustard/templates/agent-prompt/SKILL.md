# Agent Prompt Template

Orchestrator fills `{placeholders}` before dispatch. Agent receives the rendered version.

---

## Compact Template (custom agent — role already defined in agent)

Use when `.claude/agents/{subproject}-impl.md` exists. Role, boundary, return format, and validation are already in the agent definition — prompt only needs references + entity + task.

```
## CONTEXT
1. Read `{subproject}/CLAUDE.md` — guards, stack, key paths
2. Read `{subproject}/.claude/commands/guards.md` — mandatory rules

## REFERENCE
{reference_files}

## ENTITY
{entity_info}

## SKILLS
Your available skills are listed in the system. Before implementing, check if any skill matches your task — read its SKILL.md for patterns and examples.
Key skills for this task: {recommended_skills}
If a skill has `references/` files, read them only when you need concrete code examples.

## WEB VALIDATION
When in doubt about API usage, library version, or implementation pattern: search the web for the latest documentation before implementing. Only proceed when 100% confident.

## EFFICIENCY
- Absolute paths — NEVER cd
- Chain kill+build in single Bash
- Max 3 build attempts → STOP + report

{retry_context}

## TASK
{task_steps}
```

---

## Full Template (fallback — general-purpose agent, no custom agent)

Use when `.claude/agents/{subproject}-impl.md` does NOT exist.

```
## STEP 0: READ CONTEXT
1. `{subproject}/CLAUDE.md` — guards, stack, key paths
2. `{subproject}/.claude/commands/guards.md` — mandatory rules
3. `{subproject}/.claude/commands/notes.md` — project-specific notes

## REFERENCE MODULE
{reference_files}

## GUARDS (verify in return)
{guards_summary}

## ENTITY REGISTRY
{entity_info}

## SKILLS
Your available skills are listed in the system. Before implementing, check if any skill matches your task — read its SKILL.md for patterns and examples.
Key skills for this task: {recommended_skills}
If a skill has `references/` files, read them only when you need concrete code examples.

## WEB VALIDATION
When in doubt about API usage, library version, or implementation pattern: search the web for the latest documentation before implementing. Only proceed when 100% confident.

## ROLE: {role} — {boundary}
Validate: {validate_command}
Return: {return_sections}

## EFFICIENCY RULES
- Shell state does NOT persist between Bash calls — ALWAYS use absolute paths, NEVER cd
- Build: {build_command}
- Read each file ONCE — trust your edit
- Max 3 build attempts/step. After 3rd: STOP and report error.

{retry_context}

## TASK — Execute in order
{task_steps}
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
