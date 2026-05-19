# /scan - Agnostic Code Analyzer

> Discovers subprojects, dispatches one Task agent per subproject to analyze the codebase, then refreshes the registry, validates skills, and runs the security scan.

## Trigger

`/scan`, `/scan <subproject>`, `/scan --force`, `/scan <subproject> --force`

## Flags

- `--force` — bypasses incremental skip; rescans every subproject and regenerates `<!-- mustard:generated -->` artifacts. Without it, subprojects with unchanged source hash and no git dirty are skipped.

## Process

**1. Pre-dispatch.** Run `node .claude/scripts/scan/orchestrate.js [<subproject>] [--force]`. Parse the JSON it prints. The script handles: subproject discovery, incremental hash comparison, stale cleanup, bootstrap of foundational files (`.claude/CLAUDE.md`, root `CLAUDE.md`, `entity-registry.json`, per-subproject `CLAUDE.md`), Project Structure table refresh, agent file generation (`.claude/agents/{name}-impl.md` and `-explorer.md`), product-doc frontmatter, and rendering the per-subproject agent prompt.

**2. Dispatch agents.** For each item in `dispatch[]`, fire one `Task(general-purpose)` in a single message (parallel calls). Pass `agentPrompt` as the literal prompt — it already contains the EVIDENCE RULE, the per-subproject context, and all step instructions inline. Never `run_in_background: true`. If `dispatch[]` is empty, skip to step 3.

**3. Post-dispatch.** Run `node .claude/scripts/scan/finalize.js`. This refreshes the entity registry (`sync-registry.js --force`), updates the detect cache (`sync-detect.js`), validates generated skills (`skill-validate.js --factual`), and runs the security scan. Surface any `errors[]` or `warnings[]` from the JSON output.

## Return Format

```json
{
  "scanned": ["{subproject-1}", "{subproject-2}"],
  "skipped": ["{subproject-3}"],
  "generated": ["CLAUDE.md", ".claude/agents/api-impl.md"],
  "skills_generated": { "{subproject-1}": ["api-endpoint-pattern"] },
  "security": { "findings": 0 },
  "errors": []
}
```

## Fallback Mode

If `node .claude/scripts/scan/orchestrate.js` fails to run (script missing, Node error, JSON parse failure):

1. Run `node .claude/scripts/sync-detect.js --no-cache` directly. Parse its `subprojects[]`.
2. For each subproject, dispatch one `Task(general-purpose)` with this minimal prompt:
   ```
   Scan subproject {name} at {path}. Read {path}/CLAUDE.md.
   Analyze the source code, document patterns in {path}/.claude/commands/*.md
   (with the <!-- mustard:generated --> header), and emit one skill per
   reusable pattern in {path}/.claude/skills/{skill-name}/. Each skill must
   reference real files via Glob/Read; skip any skill you cannot back with
   ≥3 real files. No fenced code in SKILL.md body.
   ```
3. Run `node .claude/scripts/sync-registry.js --force` manually.
4. Report which step failed in your final message so the user knows.

This keeps `/scan` operational even if the orchestrator scripts are broken.

## Execution Rules

- **No confirmation prompts** — `/scan` is the approval. Proceed autonomously.
- **Read before Write/Edit** — only relevant in fallback mode (the orchestrator scripts handle reads themselves).
