# Wave 1 — Size-Gate Infrastructure

> Reference: `../wave-plan.md`

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Wave: 1/3
### Checkpoint: 2026-04-23

## Summary

Adicionar 2 hooks (`spec-size-gate`, `skill-size-gate`) + estender `skill-validate.js` com thresholds de linhas (warn @200, strict-warn @400, block @500). Default modo `warn` em todos. Wires em `templates/settings.json`. Não toca em SKILL.md/spec.md existentes — só observa.

## Entity Info

| Entity | Layer | Op |
|--------|-------|----|
| spec-size-gate | hooks | NEW |
| skill-size-gate | hooks | NEW |
| skill-validate (lines mode) | scripts | EXTEND |
| settings.json | config | WIRE |

## Files (5)

- `templates/hooks/spec-size-gate.js` (create)
- `templates/hooks/skill-size-gate.js` (create)
- `templates/hooks/__tests__/size-gates.test.js` (create)
- `templates/scripts/skill-validate.js` (modify — add `--lines` flag, thresholds)
- `templates/settings.json` (modify — register hooks + env vars)

## Boundaries

- `templates/hooks/spec-size-gate.js` — exact file
- `templates/hooks/skill-size-gate.js` — exact file
- `templates/hooks/__tests__/size-gates.test.js` — exact file
- `templates/hooks/_lib/hook-env.js` — read-only (use existing `getMode()`)
- `templates/scripts/skill-validate.js` — modify (additive)
- `templates/settings.json` — modify (additive: PreToolUse Write/Edit hooks + env block)
- **Out of bounds:** any SKILL.md, any spec.md, `.claude/` (this repo's runtime)

## Checklist

### General Agent (Wave 1)

- [x] Read `templates/hooks/_lib/hook-env.js` to understand `getMode()` / env helpers.
- [x] Read `templates/hooks/spec-hygiene.js` and `templates/hooks/close-gate.js` (5 min) for hook protocol reference.
- [x] Create `templates/hooks/spec-size-gate.js`:
  - PreToolUse, matchers `Write|Edit`
  - Trigger only when `tool_input.file_path` matches `.claude/spec/active/**/*.md` OR `.claude/spec/completed/**/*.md`
  - Count lines in the resulting content (Write: `tool_input.content`; Edit: read current file + apply `old_string`/`new_string` virtually)
  - Thresholds (constants at top): `WARN_LINES = 200`, `STRICT_WARN_LINES = 400`, `BLOCK_LINES = 500`
  - Modes via `MUSTARD_SPEC_SIZE_MODE`: `off | warn (default) | strict`
  - `warn` mode: print advisory to stderr at 200/400/500 thresholds, always allow (`permissionDecision: "allow"`)
  - `strict` mode: warn at 200/400, deny at 500 with message "spec exceeds 500 lines — split into `references/{section}.md` (see feature/SKILL.md § Spec Layout)"
  - Fail-open: any internal error → exit 0
- [x] Create `templates/hooks/skill-size-gate.js`:
  - Same pattern as above
  - Trigger when `tool_input.file_path` matches `**/SKILL.md` (any depth)
  - Same thresholds + env `MUSTARD_SKILL_SIZE_MODE`
  - Skip generated skills (file starts with `<!-- mustard:generated -->`) in warn mode but still apply in strict (generated skills should ALSO follow the limit)
- [x] Create `templates/hooks/__tests__/size-gates.test.js`:
  - Test spec-size-gate: 150 lines (silent), 250 lines (warn), 450 lines (strict-warn), 550 lines (deny in strict / warn in warn)
  - Test skill-size-gate: same 4 cases for `**/SKILL.md` paths
  - Test fail-open: corrupted stdin → exit 0 (no crash)
  - Test path filter: writes to non-spec/non-SKILL files are skipped silently
  - Use `bun test` style consistent with `templates/hooks/__tests__/hooks.test.js`
- [x] Extend `templates/scripts/skill-validate.js`:
  - Add `--lines` flag: when present, after structural validation, check line count of each SKILL.md body
  - Add 3 constants: `WARN_LINES = 200`, `STRICT_WARN_LINES = 400`, `BLOCK_LINES = 500`
  - Honor `MUSTARD_SKILL_VALIDATE_LINES_MODE` env (`off | warn | strict`, default `warn`)
  - In `--lines` strict mode: exit code 1 if any skill > BLOCK_LINES
  - Output (text + JSON) shows per-skill: `lineCount`, `tier` (`ok | warn | strict-warn | block`)
  - Update header docstring to document new flag + env
- [x] Modify `templates/settings.json`:
  - Add `MUSTARD_SPEC_SIZE_MODE`, `MUSTARD_SKILL_SIZE_MODE`, `MUSTARD_SKILL_VALIDATE_LINES_MODE` to `env` block (default `warn`)
  - Register `spec-size-gate.js` under `hooks.PreToolUse` with matcher `"Write|Edit"`, timeout 5000
  - Register `skill-size-gate.js` under `hooks.PreToolUse` with matcher `"Write|Edit"`, timeout 5000
  - Validate JSON parses (`node -e "JSON.parse(...)"`)
- [x] Run tests: `bun test templates/hooks/__tests__/size-gates.test.js` (22/22 pass)
- [x] Run regression: `bun test templates/hooks/__tests__/hooks.test.js` (103/103 pass)
- [x] Build/type-check (n/a — pure Node.js, no TS)

## Acceptance Criteria

- [x] AC-1: New hook tests pass — Command: `bun test templates/hooks/__tests__/size-gates.test.js`
- [x] AC-2: Existing hook tests still pass — Command: `bun test templates/hooks/__tests__/hooks.test.js`
- [x] AC-3: settings.json is valid JSON with both hooks registered — Command: `node -e "const s=JSON.parse(require('fs').readFileSync('templates/settings.json','utf8'));const m=JSON.stringify(s.hooks.PreToolUse);if(!m.includes('spec-size-gate')||!m.includes('skill-size-gate'))process.exit(1)"`
- [x] AC-4: skill-validate `--lines --json` returns valid JSON with `lineCount` per skill — Command: `node templates/scripts/skill-validate.js --lines --json | node -e "const j=JSON.parse(require('fs').readFileSync(0,'utf8'));if(!j.results||!j.results.every(r=>'lineCount' in r))process.exit(1)"`
- [x] AC-5: spec-size-gate fail-open on bad stdin — Command: `node -e "const{execSync}=require('child_process');try{execSync('node templates/hooks/spec-size-gate.js',{input:'not-json'})}catch(e){process.exit(1)}"`
- [x] AC-6: skill-size-gate fail-open on bad stdin — Command: `node -e "const{execSync}=require('child_process');try{execSync('node templates/hooks/skill-size-gate.js',{input:'not-json'})}catch(e){process.exit(1)}"`

## Dependencies

- `templates/hooks/_lib/hook-env.js` — existing helper for env mode parsing
- Existing test infra: `templates/hooks/__tests__/hooks.test.js`

## Concerns

- Edit-mode line counting is approximate: `old_string`/`new_string` substitution simulated via in-memory replace (one-shot first match). For multi-line replaces with `replace_all`, the count may differ slightly from final disk state. Acceptable for warn/strict (advisory).
- `skill-size-gate` triggers on ANY `**/SKILL.md` write — including `templates/skills/skill-creator/SKILL.md` (485 lines, externally maintained). In default `warn` mode this is just an advisory print; in `strict` it would block. We document this in the hook header.
