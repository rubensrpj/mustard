<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Command Authoring Examples

## Example 1: Simple Status Command

```markdown
# /status - Consolidated Status

## Trigger
`/status`

## Description
Shows consolidated project status.

## Information
1. **Git Status** — branch, modified files, pending push
2. **Pipeline** — active pipeline, current phase
3. **Build** — last validation result
4. **Entity Registry** — entity count, last update

ULTRATHINK
```
Ref: `commands/mustard/status/SKILL.md`

## Example 2: Pipeline Command (Feature)

Key sections from the feature command:

```markdown
# /feature - Feature Pipeline

## Trigger
`/feature <feature-name>`

### ANALYZE Phase
1. Read `.claude/pipeline-config.md`
2. Read `entity-registry.json` via Grep
3. Determine layers from signals

#### Scope Detection
| Signal | Scope |
|--------|-------|
| 1-2 layers, ≤5 files | Light |
| 3+ layers, 5+ files | Full |

### PLAN Phase
Create `.claude/spec/active/{date}-{name}/spec.md`

## Rules
- NEVER implement code in Full scope
- ALWAYS create pipeline state at PLAN phase
- Light scope + user chose "implement now" → EXECUTE inline

ULTRATHINK
```
Ref: `commands/mustard/feature/SKILL.md`

## Example 3: Git Command with Action Dispatch

```markdown
# /git - Git Operations

## Trigger
`/git <action>`

## Actions
| Action | Description |
|--------|-------------|
| `sync` | Pull parent into current |
| `commit` | Create commit (no push) |
| `push` | Sync + commit + push |

## Step 0 — Resolve Parent
cat mustard.json → match branch against flow patterns

## Behavior
- ZERO confirmations
- Minimize Bash calls — chain with &&
- Submodules BEFORE parent

ULTRATHINK
```
Ref: `commands/mustard/git/SKILL.md`
