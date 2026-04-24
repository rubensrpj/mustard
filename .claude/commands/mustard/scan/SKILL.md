# /scan - Agnostic Code Analyzer

> ALWAYS before making any change. Search on the web for the newest documentation and only implement if you are 100% sure it will work.

## Trigger

`/scan`, `/scan <subproject>`, `/scan --force`, `/scan <subproject> --force`

## Flags

- `--force` — discards all `<!-- mustard:generated -->` content and regenerates from scratch. Bypasses incremental skip, always regenerates CLAUDE.md and registry. → See `../../../refs/scan/scan-protocol.md` for full semantics.

## Execution Model

**CRITICAL — Context Protection:** Orchestrator MUST NOT analyze directly. ALL analysis delegated to Task agents.

**CRITICAL — Read-Before-Write:** Every `Write`/`Edit` on an existing file MUST be preceded by a `Read` call in the same context. Applies to `.claude/CLAUDE.md`, root `CLAUDE.md`, `.claude/docs/*.md`, and subproject `CLAUDE.md` edits.

→ See `../../../refs/scan/scan-protocol.md` for full execution model details.

## Process

**1. Discover & Incremental Detection** — Read old cache → `sync-detect.js --no-cache` → compare hashes + gitDirty → build agent list (skip unchanged; process mismatch/dirty). → See `../../../refs/scan/scan-protocol.md §Step 1`.

**2.5. Cleanup Stale Subprojects** — Remove directories, cache entries, agent files, skills, and registry entries for subprojects no longer detected. → See `../../../refs/scan/scan-protocol.md §Step 2.5`.

**2.6. Bootstrap (if needed)** — Fast-path: skip if root `CLAUDE.md` + `entity-registry.json` exist and `--force` not active. Otherwise create `.claude/CLAUDE.md`, root `CLAUDE.md`, `entity-registry.json`, and per-subproject `CLAUDE.md`. → See `../../../refs/scan/scan-protocol.md §Step 2.6`.

**2.7. Scan Product Docs** — If `.claude/docs/` has `.md` files, inject YAML frontmatter (name, description, topics, scanned-at). Orchestrator does this inline (no Task agent needed). → See `../../../refs/scan/scan-protocol.md §Step 2.7`.

**3. Launch Agents** — Launch ALL agents in a SINGLE message (parallel tool calls). Each agent receives the EVIDENCE RULE prompt. **Never `run_in_background: true`.** → See `../../../refs/scan/evidence-rules.md` for full agent prompt template and EVIDENCE RULE 1-5.

**4. Update CLAUDE.md files** — Regenerate `.claude/CLAUDE.md` (always overwrite); update root `CLAUDE.md` (Project Structure table, commands, Ignore Paths). → See `../../../refs/scan/scan-protocol.md §Step 4`.

**4.5. Generate Agents** — Generate `{subproject.name}-impl.md` and `{subproject.name}-explorer.md` per subproject. → See `../../../refs/scan/scan-protocol.md §Step 4.5`.

**4.6. Generate Granular Skills** — One skill per cluster (skill-creator methodology). → See `../../../refs/scan/scan-protocol.md §Step 4.6` and `scan-format.md §10`.

**4.7. Refresh Registry** — `node .claude/scripts/sync-registry.js --force`. → See `../../../refs/scan/scan-protocol.md §Step 4.7`.

**5. Update Cache** — `node .claude/scripts/sync-detect.js` (with cache write). → See `../../../refs/scan/scan-protocol.md §Step 5`.

**6. Validate Skills** — `node .claude/scripts/skill-validate.js --factual`. Control: `MUSTARD_SKILL_VALIDATE_MODE=strict|warn|off`. → See `../../../refs/scan/evidence-rules.md §Validate Skills Step`.

**Security Scan** — Run after step 3 or via `/scan --security`. `node .claude/scripts/security-scan.js "$PROJECT_DIR"`. → See `../../../refs/scan/scan-protocol.md §Security Scan Phase`.

## Return Format

```json
{
  "scanned": ["{subproject-1}", "{subproject-2}"],
  "generated": { "{subproject-1}": ["stack.md", "modules.md", "guards.md"] },
  "skills_generated": { "{subproject-1}": ["api-endpoint-wiring", "api-service-base"] },
  "errors": []
}
```
