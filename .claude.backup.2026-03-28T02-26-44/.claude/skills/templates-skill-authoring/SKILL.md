---
name: templates-skill-authoring
description: "Pattern for writing foundation and subproject skills with YAML frontmatter,
  pushy descriptions, and references. Use when creating a new skill, writing a
  SKILL.md, adding skill references, or the user says 'new skill', 'create skill',
  'add pattern skill', 'write foundation skill'."
---
<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->

# Skill Authoring Pattern

Skills are auto-loaded by Claude based on task description matching the skill's `description` field.

## Pattern

### File Convention
- Location: `skills/{skill-name}/SKILL.md` (foundation) or `{sub}/.claude/skills/{skill-name}/SKILL.md` (subproject)
- YAML frontmatter: `name`, `description`, optional `disable-model-invocation`
- `<!-- mustard:generated -->` goes AFTER closing `---` (never before opening `---`)
- Max 500 lines (ideally under 200)

### Description Writing (Critical)

Descriptions are the PRIMARY trigger mechanism. They must be "pushy":

**Good:**
```yaml
description: "Pattern for .NET Minimal API endpoints with validation and auth.
  Use when creating new routes, adding HTTP verbs, wiring endpoints, or the user
  says 'add endpoint', 'new route', 'expose via API'."
```

**Bad:**
```yaml
description: "API patterns"
```

### SKILL.md Structure

```markdown
---
name: {skill-name}
description: "{What}. {When — be specific and pushy}."
---
<!-- mustard:generated at:{ISO} role:{role} -->

# {Skill Title}

{1-2 sentence summary.}

## Pattern

{Concise rules, file conventions, key constraints.}

## Example

{5-10 line code example — happy path only.}
Ref: `{path/to/real/file}`

## References

For full code examples with variants:
> Read `references/examples.md`
```

### references/examples.md

- 2-3 real examples at different complexity levels
- Always include `Ref: path/to/file.ext`
- Max 15 lines per example
- Starts with `<!-- mustard:generated -->` header

## Example

```yaml
---
name: commit-workflow
description: Git commit strategy, submodule-aware, budget <=15 API calls.
disable-model-invocation: true
---
<!-- mustard:generated -->

# Commit Workflow

> Git strategy for mono-repo with submodules.

## Strategy
1. `git status` — see all changes
2. Group changes by subproject
3. Per subproject: `git add` specific files -> `git commit`
```
Ref: `skills/commit-workflow/SKILL.md`

## References

For full code examples with variants:
> Read `references/examples.md`
