# /scan — Agent Analysis & Format Rules

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

> Instructions for Task agents performing `/scan` analysis on a single subproject.
> The orchestrator passes: subproject name, path, role, stack summary, and (when available) pre-extracted clusters + sample code.

> **Default**: the agent prompt rendered by `orchestrate.js` covers the full protocol inline, including enriched clusters and sample code. Refs in `../../../refs/scan/` are detail-level fallbacks — read them ONLY if a specific instruction is ambiguous, not as a routine first step.

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

Generate `recipes.md` from import chains and module structure. One recipe per recurring type (new entity, modify, new endpoint). Format: steps with `§N` refs, reference module, task splits with deps, file hierarchy table. Ends with build check.

> Reference (optional, only if the recipe pattern is ambiguous): `../../../refs/scan/agent-recipes.md`

## 9. Agent Generation

Generate `{subproject.name}-impl.md` + `{subproject.name}-explorer.md` in root `.claude/agents/` (NOT subproject). YAML frontmatter BEFORE `<!-- mustard:generated -->`. Explorer uses haiku + read-only tools. Clean up stale leftover files from prior projects.

> Reference (optional, only if the agent template is unclear): `../../../refs/scan/agent-recipes.md`

## 10. Granular Skill Generation (skill-creator methodology)

One skill per reusable codebase convention (not per file type). Name from what codebase calls the thing. Cluster skills come from the orchestrator's injected `## Clusters detected for this subproject` block — fall back to `entity-registry.json` `_patterns[].discovered[]` only if that block is empty. Skip clusters with <3 files or noise suffixes. NO code in SKILL.md body — all code in `references/examples.md` (use the `## Sample code per cluster` block injected by the orchestrator instead of re-Reading source files).

> Reference (optional, only for skill-description writing tips and edge cases): `../../../refs/scan/skill-generation.md`
