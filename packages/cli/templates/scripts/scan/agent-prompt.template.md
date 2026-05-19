You are scanning subproject `{{name}}` at `{{path}}`.
Role: {{role}}. Stack: {{stack}}.
Absolute subproject path: `{{absSubprojectPath}}`.

{{clustersBlock}}
{{samplesBlock}}

## Goal
Document the codebase as it exists today. Generate `.md` files in `{{absSubprojectPath}}/.claude/commands/` and granular skills in `{{absSubprojectPath}}/.claude/skills/`. Update `{{absSubprojectPath}}/CLAUDE.md` with a `## Scan References` section.

## HARD CONTRACT — non-negotiable before you finish

When you stop, `{{absSubprojectPath}}/.claude/skills/` MUST contain at least one of:
- One or more `<skill-name>/SKILL.md` files (the normal case — at least one cluster met the evidence threshold), **OR**
- A single file `{{absSubprojectPath}}/.claude/skills/_no-patterns.md` (the explicit "nothing applied" case).

**If you decide no cluster qualifies for a skill** (every cluster has <3 files, only noise suffixes, or no clusters at all), you MUST write `_no-patterns.md` with this exact structure:

```
<!-- mustard:generated at:{ISO} role:{{role}} -->
# No granular skills for {{name}}

Scanned {N} files across {M} clusters. None met the evidence threshold (fileCount >= 3, non-noise suffix).

## Clusters examined
- {cluster-name}: {fileCount} files — reason skipped
- ...

## Why
{One paragraph: tiny surface area / shared library re-exporting / mostly types / etc.}
```

Returning with `skills/` empty (no SKILL.md AND no `_no-patterns.md`) is a contract violation — the orchestrator will dispatch a second agent to redo the work, doubling cost. Always emit either real skills or the placeholder.

## Execution Rules
- **Never ask the user for confirmation** — `/scan` was invoked, that is the approval. Proceed autonomously.
- **Read before Write/Edit** — Claude Code rejects writes to existing files without a prior Read in the same context.
- **Read existing knowledge first** — `{{absSubprojectPath}}/.claude/commands/*.md` and `{{absSubprojectPath}}/CLAUDE.md` are the base. Enrich them with real data.
- **All generated files in English**, with `<!-- mustard:generated at:{ISO} role:{{role}} -->` header.

{{forceBlock}}

{{budgetBlock}}

{{evidenceBlock}}

## Steps

1. **Read existing knowledge** — `{{absSubprojectPath}}/.claude/commands/*.md` and `{{absSubprojectPath}}/CLAUDE.md`.

{{toolingBlock}}

{{structureBlock}}

2. **Analyze source code** — adapted to role:
   - Stack discovery (read package manifests, extract dependencies with versions)
   - Tooling detection (build, test, lint, migrations, codegen — exact commands with flags). Use the `## Tooling detected` block above when present — only re-read source files if a command looks wrong or incomplete.
   - Pattern detection (read 3-5 representative files, identify recurring patterns)
   - Complexity classification (group features by simple/medium/complex)
   - Reference examples (one concrete file per complexity level)
   - Guard inference (DO/DON'T from patterns + existing CLAUDE.md)

3. **Write generated files to `{{absSubprojectPath}}/.claude/commands/`** — adapted to role:
   - **api**: `stack.md`, `modules.md`, `patterns.md`, `guards.md`, `recipes.md`
   - **ui**: `stack.md`, `features.md`, `patterns.md`, `guards.md`, `recipes.md`
   - **library**: `stack.md`, `exports.md`, `patterns.md`, `guards.md`, `recipes.md`
   - **infra**: `stack.md`, `patterns.md`
   - **general**: pick the subset that fits what you found.

   File budget: ≤200 lines per file. When exceeded, split by category (`patterns-crud.md`, `patterns-advanced.md`).
   Tables over prose. Inline `Ref: path/file.ext`. Code blocks max 5-8 lines.
   `guards.md` contains ONLY DO/DON'T rules — code examples belong in `patterns*.md`.

4. **Generate granular skills** in `{{absSubprojectPath}}/.claude/skills/{skill-name}/` following skill-creator methodology. ALWAYS use this absolute path — never write to relative `.claude/skills/` (the orchestrator runs from a different working directory). Remember the HARD CONTRACT above: if no cluster qualifies, write `_no-patterns.md` instead of leaving the directory empty:
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

5. **Update `{{absSubprojectPath}}/CLAUDE.md`** — Read first, then Edit:
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
  "skillsWritten": 2,
  "noPatternsMarker": false,
  "errors": []
}
```

`skillsWritten` is the count of `SKILL.md` files you wrote. `noPatternsMarker` is `true` if you wrote `_no-patterns.md` instead. **Exactly one of these must be non-zero/true** — the orchestrator validates this and surfaces a warning otherwise.
