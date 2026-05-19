You are scanning subproject `{{name}}` at `{{path}}`.
Role: {{role}}. Stack: {{stack}}.
Absolute subproject path: `{{absSubprojectPath}}`.

{{clustersBlock}}
{{samplesBlock}}

## Goal
Document the codebase as it exists today. Generate `.md` files in `{{absSubprojectPath}}/.claude/commands/` and granular skills in `{{absSubprojectPath}}/.claude/skills/`. Update `{{absSubprojectPath}}/CLAUDE.md` with a `## Scan References` section.

## Execution Rules
- **Never ask the user for confirmation** — `/scan` was invoked, that is the approval. Proceed autonomously.
- **Read before Write/Edit** — Claude Code rejects writes to existing files without a prior Read in the same context.
- **Read existing knowledge first** — `{{absSubprojectPath}}/.claude/commands/*.md` and `{{absSubprojectPath}}/CLAUDE.md` are the base. Enrich them with real data.
- **Backup before overwrite** — move ONLY files containing `<!-- mustard:generated` to `{{absSubprojectPath}}/.claude/commands/_backup/`. Files without that header are user-authored (e.g. `notes.md`) — preserve.
- **All generated files in English**, with `<!-- mustard:generated at:{ISO} role:{{role}} -->` header.

{{forceBlock}}

{{budgetBlock}}

{{evidenceBlock}}

## Steps

1. **Read existing knowledge** — `{{absSubprojectPath}}/.claude/commands/*.md` and `{{absSubprojectPath}}/CLAUDE.md`.

2. **Backup** — move generated files (those with the `<!-- mustard:generated` marker) from `{{absSubprojectPath}}/.claude/commands/` to `{{absSubprojectPath}}/.claude/commands/_backup/`. Create `_backup/` if missing.

3. **Ensure `{{absSubprojectPath}}/.claude/commands/notes.md` exists** — if not, create with H1 `Notes: {{name}} ({{role}})`, blockquote description, and sections `## Mandatory Patterns`, `## Known Pitfalls`, `## Observations`.

4. **Analyze source code** — adapted to role:
   - Stack discovery (read package manifests, extract dependencies with versions)
   - Structure mapping (top-level folders → project layout)
   - Tooling detection (build, test, lint, migrations, codegen — exact commands with flags)
   - Pattern detection (read 3-5 representative files, identify recurring patterns)
   - Complexity classification (group features by simple/medium/complex)
   - Reference examples (one concrete file per complexity level)
   - Guard inference (DO/DON'T from patterns + existing CLAUDE.md)

5. **Write generated files to `{{absSubprojectPath}}/.claude/commands/`** — adapted to role:
   - **api**: `stack.md`, `modules.md`, `patterns.md`, `guards.md`, `recipes.md`
   - **ui**: `stack.md`, `features.md`, `patterns.md`, `guards.md`, `recipes.md`
   - **library**: `stack.md`, `exports.md`, `patterns.md`, `guards.md`, `recipes.md`
   - **infra**: `stack.md`, `patterns.md`
   - **general**: pick the subset that fits what you found.

   File budget: ≤200 lines per file. When exceeded, split by category (`patterns-crud.md`, `patterns-advanced.md`).
   Tables over prose. Inline `Ref: path/file.ext`. Code blocks max 5-8 lines.
   `guards.md` contains ONLY DO/DON'T rules — code examples belong in `patterns*.md`.

6. **Generate granular skills** in `{{absSubprojectPath}}/.claude/skills/{skill-name}/` following skill-creator methodology. ALWAYS use this absolute path — never write to relative `.claude/skills/` (the orchestrator runs from a different working directory):
   - One conceptual pattern per skill (NOT one file per skill).
   - Name from what the codebase calls the thing — folder `Resolvers/` → `{{name}}-resolver-pattern`. Never import vocabulary the codebase does not use.
   - Use the clusters from the `## Clusters detected for this subproject` block above. Each cluster in that block represents a reusable convention. Skip clusters with fewer than 3 files OR noise suffixes (`Test`/`Mock`/`Spec`). If the block is empty (first run, no registry yet), fall back to reading `.claude/entity-registry.json` and iterating `_patterns[{stackId}].discovered[]`.
   - SKILL.md frontmatter:
     ```yaml
     ---
     name: {skill-name}
     description: "{What it does}. Use when {trigger phrase 1}, {trigger phrase 2}. Even if the user just says '{casual phrase}'."
     source: scan
     ---
     <!-- mustard:generated at:{ISO} role:{{role}} -->
     ```
   - SKILL.md body sections: `## Convention` (cluster fields as bullets), `## Real examples in this codebase` (file paths, verified), `## References` (pointer to `references/examples.md`).
   - **NO fenced code blocks in SKILL.md body.**
   - `references/examples.md` extracts verbatim code (≤80 lines per example).

7. **Update `{{absSubprojectPath}}/CLAUDE.md`** — Read first, then Edit:
   - `## Scan References` section listing generated files
   - `## Guards` populated with key guards from analysis
   - `## Stack` filled if empty
   - `## Commands` MUST include ALL detected tooling (build, test, migrations, codegen, lint)
   - `## Recommended Skills` listing the generated pattern skills

## Return Format

```json
{
  "subproject": "{{name}}",
  "generated": ["stack.md", "patterns.md", "guards.md"],
  "skills": ["{{name}}-resolver-pattern", "{{name}}-handler-pattern"],
  "errors": []
}
```
