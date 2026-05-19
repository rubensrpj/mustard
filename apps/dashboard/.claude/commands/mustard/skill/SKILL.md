# /skill - Skill Manager

> Manage skills: install, create, list, remove, optimize, eval.

## Trigger

`/skill <action> [args]`

## Actions

| Action | Usage | Description |
|--------|-------|-------------|
| `install` | `/skill install <source>` | Install skill from local path or GitHub |
| `create` | `/skill create <name>` | Create new skill using skill-creator |
| `list` | `/skill list` | List all installed skills |
| `remove` | `/skill remove <name>` | Remove a skill |
| `optimize` | `/skill optimize <name>` | Optimize skill description for better triggering |
| `eval` | `/skill eval <name>` | Run eval loop on a skill |
| `update` | `/skill update skill-creator` | Update skill-creator from anthropics/skills repo |

## install

Install a skill from a source into `.claude/skills/{name}/`.

### Sources

| Format | Example |
|--------|---------|
| Local path | `/skill install ./my-skills/api-caching/` |
| GitHub (sparse) | `/skill install github:anthropics/skills/skills/pdf` |
| GitHub (full repo) | `/skill install github:owner/repo` (installs all skills found) |

### Flow

1. **Resolve source** — determine if local path or GitHub URL
2. **For GitHub**: sparse clone to temp, copy skill folder to `.claude/skills/{name}/`
3. **For local**: copy skill folder to `.claude/skills/{name}/`
4. **Validate**: run `python .claude/skills/skill-creator/scripts/quick_validate.py .claude/skills/{name}/SKILL.md`
5. **Report**: show installed skill name, description, and path

### GitHub Sparse Clone

```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/{owner}/{repo}.git /tmp/skill-install-{name}
cd /tmp/skill-install-{name}
git sparse-checkout set {path}
cp -r {path} .claude/skills/{name}/
```

## create

Create a new skill interactively using the skill-creator.

### Flow

1. **Invoke skill-creator** — use the `skill-creator` skill which is installed at `.claude/skills/skill-creator/`
2. The skill-creator handles: capture intent → interview → write SKILL.md → test cases → eval → iterate
3. Skill is created directly in `.claude/skills/{name}/`

### Usage

```
/skill create api-caching
```

This triggers the skill-creator methodology:
- Ask what the skill should do
- When should it trigger
- Expected output format
- Write draft SKILL.md
- Create test prompts
- Run eval loop (optional)
- Optimize description (optional)

## list

List all installed skills with source, role hints, and description.

### Flow

1. Glob `.claude/skills/*/SKILL.md`
2. Parse YAML frontmatter of each
3. Display table:

```
| Name | Source | Description |
|------|--------|-------------|
| design-craft | manual | Unified design skill for all UI work... |
| api-endpoint-wiring | scan | Pattern for .NET Minimal API endpoints... |
| skill-creator | manual | Create and improve skills... |
```

## remove

Remove a skill and its directory.

### Flow

1. Check `.claude/skills/{name}/` exists
2. Check if `source: scan` → warn: "This skill will be regenerated on next /scan. Remove anyway?"
3. Delete `.claude/skills/{name}/` directory
4. Report removed

## optimize

Optimize a skill's description for better triggering accuracy.

### Flow

1. Read `.claude/skills/{name}/SKILL.md`
2. Use skill-creator's description optimization:
   - Generate 20 trigger eval queries (should-trigger + should-not-trigger)
   - Review with user
   - Run optimization loop: `python .claude/skills/skill-creator/scripts/run_loop.py --eval-set <eval.json> --skill-path .claude/skills/{name} --max-iterations 5`
3. Apply best_description to skill frontmatter
4. Report before/after scores

## eval

Run the skill-creator eval loop on a skill.

### Flow

1. Read `.claude/skills/{name}/SKILL.md`
2. Use skill-creator eval methodology:
   - Create 2-3 test prompts
   - Run with-skill and baseline subagents
   - Grade results
   - Aggregate benchmark
   - Launch viewer for user review
   - Iterate based on feedback
3. Report results

## update

Update the skill-creator from the anthropics/skills repo.

### Flow

1. Sparse clone `anthropics/skills` repo
2. Copy `skills/skill-creator/` to `.claude/skills/skill-creator/`
3. Report updated version

## Rules

- NEVER delete skills that are `source: manual` without user confirmation
- ALWAYS validate SKILL.md after install with quick_validate.py
- ALWAYS use skill-creator for `/skill create` — don't write skills from scratch
- `/skill optimize` and `/skill eval` require Python 3 and `claude` CLI
- `source:` field semantics (TERRITORIAL):
  - `/scan` agents (§4.6, §10) write `source: scan` ONLY — never touch `source: manual`.
  - `/skill install`, `/skill create`, skill-creator write `source: manual` ONLY — never touch `source: scan`.
  - Missing `source:` → treat as `manual` (conservative, protects user edits).
