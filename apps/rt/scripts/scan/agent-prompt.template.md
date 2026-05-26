<!-- mustard:generated -->
You are interpreting the structural scan of subproject `{{name}}` at `{{path}}`.
Role: {{role}}. Absolute path: `{{absSubprojectPath}}`.

## What you have already
The mechanical scan already ran. Trust the digest below; do not re-walk it.

{{structureBlock}}
{{toolingBlock}}
{{clustersBlock}}
{{samplesBlock}}

## What you must produce
Write atomically under `{{absSubprojectPath}}/.claude/`. Each file starts with
`<!-- mustard:generated -->` (after frontmatter when present). Caps enforced by
`mustard-rt run scan-md-validate`.

| Path | Cap | Purpose |
|---|---|---|
| `commands/patterns.md` | 150 lines | shapes derived from the digest clusters |
| `commands/notes.md` | 80 lines | open questions + intentional gaps |
| `skills/<cluster>/SKILL.md` | 60 lines | one skill per qualifying cluster |

A cluster qualifies when the digest shows `fileCount >= 3` and a non-noise
label (skip `test`, `mock`, `spec`).

## HARD CONTRACT
`{{absSubprojectPath}}/.claude/skills/` MUST contain at least one of:
- one or more `<cluster>/SKILL.md` files, OR
- a single `_no-patterns.md` file when no cluster qualifies.

Empty `skills/` is a contract violation — the orchestrator re-dispatches.

## Language policy
English output. Borrow vocabulary from the subproject's filenames and the
digest's cluster labels — never introduce names the digest did not list.

## SKILL.md shape
```
---
name: <cluster-label>-pattern
description: "<what the cluster encodes>. Use when <trigger 1>, <trigger 2>."
source: scan
---
<!-- mustard:generated -->
## Convention
- <bullet derived from cluster fields>

## Real examples in this codebase
- `<verified path from samples>` — <one line>

## References
- See `references/examples.md` for verbatim code.
```

No fenced code blocks inside SKILL.md body — extracts go in `references/examples.md`.
## patterns.md shape
One H2 per cluster (bullets for `folderPattern`, `samples`, `memberSuffixes`),
plus a final `## Conventions` H2 capturing the dominant naming bucket.

## notes.md shape
Bullets for decisions not inferable from the digest. Paragraphs <=4 lines.
## Wirelinks
Cross-references use `[[<sub>.<kind>.<slug>]]` (`kind` in `conv`, `skill`,
`recipe`, `entity`, `command`, `ref`). Each must resolve against
`.claude/graph/index.md`.

## Return Format
```json
{
  "subproject": "{{name}}",
  "generated": ["commands/patterns.md", "commands/notes.md"],
  "skills": ["<cluster>-pattern"],
  "skillsWritten": 1,
  "noPatternsMarker": false,
  "errors": []
}
```
