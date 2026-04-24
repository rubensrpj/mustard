# Granular Skill Generation Reference

> Detail for `/scan` §10: how to generate subproject skills from detected patterns and the entity registry cluster.

## 10. Granular Skill Generation (skill-creator methodology)

Instead of monolithic pattern files, generate **granular skills** — one conceptual pattern per skill.
Follow the [skill-creator](https://github.com/anthropics/skills) methodology for SKILL.md structure.

### Decomposition Rules

Each detected pattern becomes its own skill. Group by **conceptual unit**, not by file. The agent derives both the skill name and its scope from what the codebase actually shows — no fixed list of technologies, no predetermined taxonomy.

**Naming**: `{subproject-short}-{kebab-case-concept}` — the concept is whatever the codebase itself calls the thing. If the project has a folder called `Resolvers/`, the skill is `{sub}-resolver-pattern`. If it has `composables/`, it's `{sub}-composable-pattern`. If it has `Handlers/`, it's `{sub}-handler-pattern`. Never import vocabulary the codebase does not use.

**Scope**: one skill per reusable convention the agent would hand to a future agent implementing "add one more like these". If the pattern is a one-off file, it is not a convention — skip it.

**Anti-patterns to avoid**:
- Emitting a skill for every file type (`.ts`, `.css`, `.json` → three skills = noise)
- Emitting a skill for generic cross-cutting concerns (logging, error handling) unless the codebase has a distinctive, repeated shape for them
- Naming a skill after a library the codebase uses but whose convention is entirely the library's default (e.g. "using React hooks as documented" is not a codebase convention)

### Cluster Skills from the Registry (mandatory)

After generating the conceptual skills above, **also** read `.claude/entity-registry.json` and iterate `_patterns[{stackId}].discovered[]` for each detected stack. Each cluster entry looks like:

```json
{
  "suffix": "Service",
  "fileCount": 7,
  "folders": ["src/Services", "src/Modules/Auth/Services"],
  "samples": ["src/Services/UserService.cs", "..."],
  "commonBaseClass": "BaseService",
  "commonInterfaces": ["IService"]
}
```

For each cluster that represents a **reusable convention** (skip one-offs, test-only files, or trivial groupings), emit a skill named `{sub}-{suffix-slug}-pattern` (e.g. `backend-service-pattern`, `frontend-component-pattern`). SKILL.md body:

- `## Pattern` — enumerate `suffix`, `fileCount`, `folderPattern`, `commonBaseClass`, `commonInterfaces` as bullets (fields that exist in the cluster — do not invent).
- `## Rules` — DO/DON'T derived from the cluster (naming, folder placement, base class usage).
- `## Samples in this project` — bullet list of the `samples` file paths.
- `## References` — pointer to `references/examples.md`.

**Agent judgment filter**: do NOT blindly emit one skill per cluster. If a cluster has fewer than ~3 files OR the suffix is generic noise (e.g. `Test`, `Mock`, `Spec`), skip it. The goal is reusable conventions, not coverage theater.

`_patterns.folderFrequency` provides a stopword source for distinctive-keyword extraction: segments appearing in ≥60% of all folders are structural noise and should be ignored when describing the cluster.

### Skill Structure

```
.claude/skills/{skill-name}/
├── SKILL.md              → Pattern instruction (<500 lines, ideally <200)
└── references/            → Optional: concrete code examples from codebase
    └── examples.md        → Real code snippets with file paths
```

### SKILL.md Format (skill-creator standard)

**CRITICAL — NO CODE IN SKILL.md.** The SKILL.md body describes the pattern in prose + bullet lists + file references. **Never** embed code blocks, language-specific stubs, or synthesized examples (no fake `class Order { ... }`, no TypeScript snippet, no SQL, nothing). All concrete code lives in `references/examples.md`, extracted from real source files.

```yaml
---
name: {skill-name}
description: "{What it does}. {When to use it — be specific and 'pushy'}.
  Use when {trigger phrase 1}, {trigger phrase 2}, or {trigger phrase 3}.
  Even if the user just says '{casual phrase}'."
source: scan
---
<!-- mustard:generated at:{ISO} role:{role} -->

# {Skill Title}

> Pattern detected in this project.

## Convention

- Folder: `{detected folder}`
- {other fields present in the pattern — enumerate dynamically, do not assume}
- Naming: `{detected naming convention}`

## Real examples in this codebase

- `{EntityName}` — `{path/to/real/file.ext}`
- `{OtherEntity}` — `{path/to/other/file.ext}`

## References

See `references/examples.md` for extracted code.
```

**Important:** The `## Convention` section lists detected fields from the registry pattern directly (folder, base class, naming, interfaces, etc.). The `## Real examples` section lists actual file paths from the registry. Concrete code — if any is needed at all — belongs only in `references/examples.md`, extracted from real source files (never handwritten).

### references/examples.md Format

```markdown
<!-- mustard:generated at:{ISO} -->

# {Pattern} — real examples from this codebase

## {EntityName}
Source: `{path/to/real/file.ext}`
\`\`\`{lang-from-extension}
{actual file content or excerpt — ≤80 lines full, or first class declaration ±20 lines}
\`\`\`
```

File extension maps to fence language: `.ts` → `typescript`, `.cs` → `csharp`, `.py` → `python`, `.dart` → `dart`, etc. Unknown extension: no language tag. If the file does not exist (stale registry), skip that entry silently.

### Description Writing Guidelines (from skill-creator)

Descriptions are the PRIMARY trigger mechanism — Claude uses them to decide which skills to load.

**DO:**
- Be specific about what the skill covers AND when to use it
- Be "pushy" — include casual phrases users might type
- Include edge cases and near-misses
- Example: "Pattern for .NET Minimal API endpoints with validation, auth metadata, and ApiResponse<T>. Use when creating new routes, adding HTTP verbs, wiring endpoints, or the user says 'add endpoint', 'new route', 'expose via API'."

**DON'T:**
- Generic descriptions like "API patterns"
- Only formal trigger phrases — include casual ones
- Describe implementation details in the description — save those for the body

### references/ Files

Extract concrete code examples from the codebase into `references/examples.md`:
- Include 2-3 real examples at different complexity levels
- Always include `Ref: path/to/file.ext` for each
- Keep each example ≤15 lines of code
- Agent reads these ONLY when needing concrete implementation reference

### Subproject-Level Skills

For each generated skill, ALSO create in `{subproject}/.claude/skills/{skill-name}/`:
- Same content as root-level skill
- Purpose: enables standalone use outside the monorepo
- Mark with `<!-- mustard:generated -->` same as root versions

### Skills Location

Skills are generated ONLY in `{subproject}/.claude/skills/{skill-name}/` (NOT in root `.claude/skills/`).
This keeps subproject-specific knowledge self-contained and avoids duplication.
