# /scan — Agent Analysis & Format Rules

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

> Instructions for Task agents performing `/scan` analysis on a single subproject.
> The orchestrator passes: subproject name, path, role, and stack summary.

## Language Rule

**ALL generated `.md` files MUST be written in English.** Only exception: `notes.md` files that already exist with user-written content — those are never overwritten.

## Execution Rules

**NEVER ask the user for confirmation** during scan generation. The user already invoked `/scan` — that IS the approval. Proceed autonomously for every action:

- File writes, overwrites, and deletions: execute without prompting
- Backups to `_backup/`: execute without prompting
- `CLAUDE.md` updates (root and per-subproject): execute without prompting
- Stale file/skill cleanup: execute without prompting (safety checks are enough)
- Skill directory creation under `.claude/skills/`: execute without prompting

If an action fails, surface the error in the return format — do NOT stop to ask what to do. The orchestrator decides recovery at the end.

## Read-Before-Write Rule

Claude Code's `Write`/`Edit` tools reject calls on existing files that have not been `Read` in the current context (`File has not been read yet. Read it first before writing to it.`).

Before every `Write` (full overwrite) or `Edit` (patch) on an existing path:
1. Call `Read` on that path (any offset is enough to satisfy the contract).
2. Then issue the `Write`/`Edit`.

This applies to every step below that touches existing files — backups, notes.md updates, generated command files that were not fully removed by backup, and the subproject `CLAUDE.md` updates in §7. When in doubt, `Read` first.

## 1. Read Existing Knowledge

Before analyzing code, read existing files as a starting point:
- `{subproject}/.claude/commands/*.md` — partial knowledge already documented
- `{subproject}/CLAUDE.md` — patterns, guards, references

These files contain patterns refined over time. Use them as **base** and enrich with real data.

## 2. Backup

Move **only generated** command files to backup:
```
{subproject}/.claude/commands/*.md → {subproject}/.claude/commands/_backup/
```
Create `_backup/` if it doesn't exist. Overwrite previous backups.

**CRITICAL**: Only backup/overwrite files that contain `<!-- mustard:generated` in their first line. Files **without** this header are **manual notes** (e.g., `notes.md`) and MUST be preserved — never move, delete, or overwrite them.

## 3. Ensure Notes File

Check if `{subproject}/.claude/commands/notes.md` exists. If NOT, create with H1 `Notes: {Name} ({Role})`, blockquote description, and sections: `## Mandatory Patterns`, `## Known Pitfalls`, `## Observations`.

## 4. Analyze Source Code

Perform analysis adapted to the detected role. Decide **what to read and generate** based on what you find — not from a fixed template.

### General analysis steps (all roles):

1. **Stack discovery**: Read package manifest → extract dependencies with real versions
2. **Structure mapping**: Walk top-level folders → document the project layout
3. **Tooling detection**: Detect dev commands beyond build/run — schema management (EF migrations, Drizzle, Prisma), code generation (Kubb, OpenAPI, protobuf), linting, testing, seeding. Document exact commands with flags and project paths.
4. **Pattern detection**: Read 3-5 representative files → identify recurring patterns
5. **Complexity classification**: Group features/modules by complexity (simple/medium/complex)
6. **Reference examples**: For each complexity level, pick one concrete example with file path
7. **Guard inference**: From patterns found + existing CLAUDE.md, compile rules (DOs and DON'Ts)

### Tooling detection details (step 3):

Scan for tooling signals and document **exact commands** in `stack.md` Commands + CLAUDE.md `## Commands`:

| Signal | What to detect |
|--------|---------------|
| `Microsoft.EntityFrameworkCore.Design` in `.csproj` | EF Core migrations |
| `drizzle-kit` / `prisma` in `package.json` | DB migrations |
| `generate:api` / `kubb` / `openapi-generator` in scripts | API codegen |
| DbContext in project A, Design in project B | Cross-project migration (`--project` + `--startup-project` flags) |

EF Core: detect DbContext project vs Design project → derive `dotnet ef migrations add {Name} --project {Data} --startup-project {Startup}`, `dotnet ef database update`, `dotnet ef migrations list`.

### What to generate (adapted to role):

| Role | Generated files |
|------|----------------|
| **api** | `stack.md`, `modules.md`, `patterns.md`, `guards.md`, `recipes.md` |
| **ui** | `stack.md`, `features.md`, `patterns.md`, `guards.md`, `recipes.md` |
| **library** | `stack.md`, `exports.md`, `patterns.md`, `guards.md`, `recipes.md` |
| **infra** | `stack.md`, `patterns.md` |

The agent may add or skip files as appropriate. The table is guidance, not rigid.

### File size budget

Every generated file MUST stay under **200 lines**. When exceeded, split by semantic category (e.g., `patterns-crud.md`, `patterns-advanced.md`). Each split: own `<!-- mustard:generated -->` header, `scope:reference`, name `{base}-{category}.md`, descriptive blockquote after H1.

### guards.md — rules only, no code examples

Guards contain ONLY DO/DON'T rules (tables, bullet lists). Code examples, file refs, and checklists belong in `patterns*.md`.

## 5. Generated File Format

Every generated file: `<!-- mustard:generated at:{ISO} role:{role} -->` header, H1 title, blockquote description, then tables/patterns with `Ref: path/file.ext` inline.

Rules:
- **No generic information** — only data traced from real code
- **Every pattern MUST reference** a concrete file that exists
- **Relative paths** to the subproject
- **ALL generated files MUST be in English**

## 6. Compaction Rules

Target: ~40% token reduction. Tables over prose | inline refs `Ref: path/file.ext` | code blocks max 5-8 lines | enums inline (`PENDING \| PAID`) | group by domain (4-5 groups) | no decorative headers | max 200 lines/file, split if exceeded.

## 7. Update CLAUDE.md Files

After generating command files, update the subproject's `CLAUDE.md`. **Always `Read` the file first** before any `Edit` — the Write/Edit tools will otherwise fail with `File has not been read yet`. Then:
- Add or update `## Scan References` section listing generated files with brief description
- If `## Guards` section exists, update with key guards from analysis
- If `## Stack` is empty, fill from detected stack
- **`## Commands` section MUST include ALL detected tooling commands** — not just build/run. Include migration commands, code generation, seeding, testing, linting. Every command an agent might need to execute. If tooling detection (step 3) found commands, they MUST appear here.
- **`## Recommended Skills`** — list generated pattern skills (`{subproject}-patterns-*`) + matching foundation skills from `.claude/pipeline-config.md` Skill Recommendations table. This section informs agents which skills to consult before implementation.

## 8. Implementation Recipes

Generate `recipes.md` — implementation index for the orchestrator.

Analyze import chains, module structure, and file dependencies to discover the implementation hierarchy. Generate recipes for recurring implementation types.

### Analysis: 1) Import chains → dependency levels 2) Extract sequences from `patterns*.md` 3) Build recipe per type (new entity, modify, new endpoint) 4) Pick reference modules per complexity 5) Split at natural dependency boundaries

### Format per recipe:
```markdown
## Recipe: {Type Name}
### Steps
1. {Step} → `{pattern-file}.md` §{N}
N. Build/type-check
### Reference module: {Name} | Reference files: {path/to/file1}, {path/to/file2}
### Task splits
- **{SplitName}** (steps {range}): Patterns: `{file}.md` | Depends on: {none | split | "agent:{role} completed"}
### File hierarchy
| Level | Component | Depends on |
|-------|-----------|-----------|
| 1 | {base} | — |
| N | build check | all |
```

Orchestrator uses hierarchy for: implementation ORDER, task SPLIT boundaries, CROSS-AGENT deps. Rules: exact §N refs | 3-5 reference paths | max ~10 files/split | ends with build check | hierarchy from actual imports

## 9. Agent Generation

Generate `.claude/agents/{subproject.name}-impl.md` per detected subproject (named by subproject name, NOT role):
- YAML frontmatter: name (`{subproject.name}-impl`), description, model (sonnet default), tools, memory (project)
- Body: mandatory reads, boundary, validation, return format
- Mark `<!-- mustard:generated -->` AFTER closing `---` (NEVER before opening `---` — breaks YAML frontmatter parsing)
- Explorer agent always generated (`{subproject.name}-explorer.md`, model: haiku, read-only tools, with skill refs — see `scan.md` §4.5 for full template)

| Role | Tools | Boundary |
|------|-------|----------|
| api | Read,Write,Edit,Bash,Grep,Glob | Server-side only |
| ui | Read,Write,Edit,Bash,Grep,Glob | UI + types only |
| library | Read,Write,Edit,Bash,Grep,Glob | Same as api |

Validation command: extracted from subproject CLAUDE.md → Commands section (build/type-check).

### Agent Location

Agents are generated ONLY in root `.claude/agents/` (NOT in subproject `.claude/agents/`).
The orchestrator dispatches agents from root level.

### Cleanup Stale Subproject Files

When regenerating subproject agents/skills:
- Delete files referencing wrong project names (e.g., leftovers from previous projects that don't match current subproject names)
- Clean up empty `.agent-state/` and `spec/` directories if they contain no user content
- Preserve `agent-memory/` — may contain useful investigation notes from past sessions

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
