# Evidence Rules Reference

> EVIDENCE RULE for skill generation in `/scan` agents, plus Validate Skills step.

## Agent Prompt Template (Step 3)

The full prompt sent to each Task agent launched in Step 3:

```
Read .claude/refs/scan/scan-format.md for analysis and format rules.

EVIDENCE RULE — before emitting any skill:
1. Skill must correspond to a cluster in _patterns[stack].discovered[] with fileCount >= 3.
   The cluster's `suffix` (slugified) MUST appear as a token in the skill name
   (conventionally: `{subproject-short}-{suffix-slug}-pattern`). No renaming to library brands.
2. Every path listed under `## Real examples` or `## Samples in this project` must be
   confirmed via Glob/Read. Drop entries that don't exist on disk.
3. `## Convention` fields: start from the cluster object keys (suffix, folders, fileCount,
   commonBaseClass, commonInterfaces). You MAY add fields derived from the codebase ONLY IF
   each added field is backed by a verbatim Read of at least 3 real files under the cluster's
   folders — and the value reflects the MAJORITY observed. If 7/10 samples show variant A and
   3/10 show variant B, state variant A as the convention (the majority). The field NAME and
   VALUE must come from what the code itself shows — never from library defaults, framework
   documentation, or conventions imported from other projects.
4. NO fenced code blocks in SKILL.md. All code goes to references/examples.md,
   extracted via Read from a real source file (verbatim, ≤80 lines).
5. If you cannot meet rules 1-4 for a candidate skill, SKIP it. Empty is better than invented.

**EXECUTION RULE — NO CONFIRMATION PROMPTS**: NEVER ask the user to confirm file writes,
overwrites, deletes, or directory creations. The user already invoked /scan — that IS the
approval. Proceed autonomously. If an action fails, surface the error in the return format
and move on; do NOT stop to ask what to do.

Subproject: {name}
Path: {path}
Role: {role}
Stack: {stackSummary}

FORCE MODE (only when /scan was invoked with --force):
- Before generating skills, scan {path}/.claude/skills/ and delete every subdirectory
  whose SKILL.md contains "<!-- mustard:generated" (preserve user-authored skills that
  lack that marker).
- Also delete any pre-existing _backup/ under {path}/.claude/commands/ to avoid stacking stale backups.

Tasks:
1. Read existing knowledge from {path}/.claude/commands/ and {path}/CLAUDE.md
2. Backup generated files to {path}/.claude/commands/_backup/
3. Ensure notes.md exists
4. Analyze source code following scan-format.md rules
5. Write generated files to {path}/.claude/commands/
6. Generate granular skills following scan-format.md §10 (skill-creator methodology)
7. Update {path}/CLAUDE.md with scan references
```

## EVIDENCE RULE — Expanded Detail

### Rule 1 — Cluster Backing

A skill is only valid if it maps to a cluster in `_patterns[stack].discovered[]` where `fileCount >= 3`. The cluster's `suffix` (slugified) MUST appear as a token in the skill name. Do not rename to library brands (e.g., do not use `react-query-pattern` if the cluster suffix is `service-hook`).

### Rule 2 — Path Verification

Every path listed under `## Real examples` or `## Samples in this project` in a SKILL.md MUST be confirmed via `Glob` or `Read`. Drop any path that does not exist on disk at time of generation.

### Rule 3 — Convention Fields

`## Convention` fields start from the cluster object's own keys: `suffix`, `folders`, `fileCount`, `commonBaseClass`, `commonInterfaces`. Additional fields may be added ONLY IF:
- Backed by a verbatim `Read` of at least 3 real files under the cluster's folders
- The value reflects the MAJORITY observed (if 7/10 show variant A and 3/10 show variant B → state variant A)
- Field NAME and VALUE come from what the code itself shows — never from library defaults, framework documentation, or conventions imported from other projects

### Rule 4 — No Fenced Code in SKILL.md

All code goes to `references/examples.md`, extracted via `Read` from a real source file (verbatim, ≤80 lines). SKILL.md body MUST NOT contain fenced code blocks.

### Rule 5 — Skip Over Invent

If you cannot meet rules 1-4 for a candidate skill, SKIP it. An empty skill list is better than invented or hallucinated content.

## Validate Skills Step

After all agents complete (Step 6 in scan-protocol.md):

```bash
bun .claude/scripts/skills.js validate --factual
```

### What it checks per skill

- Header: `<!-- mustard:generated -->` present
- Cluster backing: `fileCount >= 3` in registry for the skill's cluster
- Sample existence: all `## Real examples` / `## Samples in this project` paths exist on disk
- No fenced code blocks in SKILL.md body
- Reference paths exist: `references/examples.md` present if referenced

### Control

`MUSTARD_SKILL_VALIDATE_MODE=strict (default) | warn | off`

- **strict** (default): validator exit code 1 aborts the scan return. Skills are kept on disk, but user is alerted to fix them.
- **warn**: validator runs, findings reported as warnings, scan completes normally.
- **off**: validator skipped entirely.

## Verification Checklist

After scan completes, verify:

1. All skills in `{subproject}/.claude/skills/` have valid SKILL.md
2. Every generated file has `<!-- mustard:generated -->` header
3. Every generated file has a blockquote description after the H1 title
4. Every pattern references a real file
5. Old files backed up in `_backup/`
6. Each subproject's CLAUDE.md has `## Scan References`
7. Root CLAUDE.md has `## Project Structure` with all subprojects
8. `.claude/entity-registry.json` exists and is v4.0
9. Pattern skills generated from registry (entity-creation, enum-placement, route-conventions, etc.)
10. Each generated skill has valid YAML frontmatter (name + description)
11. Each skill's description is "pushy" — includes casual trigger phrases (`--factual` flag)
12. If security scan ran: findings summarized in `## Security` section of output
