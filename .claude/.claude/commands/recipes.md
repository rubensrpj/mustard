<!-- mustard:generated at:2026-03-25T00:00:00.000Z role:general -->
# Recipes: Templates

> Implementation recipes for common tasks in the templates subproject.

## Recipe: New Hook

### Steps
1. Create `hooks/{hook-name}.js` following stdin/stdout protocol → `patterns.md` P1
2. Implement logic with fail-open error handling → `patterns.md` P4
3. Choose response format: PreToolUse (`permissionDecision`) or PostToolUse (`decision`) → `patterns.md` P2, P3
4. Register in `settings.json` under correct lifecycle event with matcher and timeout
5. Add test cases in `hooks/__tests__/hooks.test.js`
6. Run tests: `bun test hooks/__tests__/hooks.test.js`

### Reference module: bash-safety.js (PreToolUse) | guard-verify.js (PostToolUse)
### Reference files: `hooks/bash-safety.js`, `hooks/guard-verify.js`, `settings.json`, `hooks/__tests__/hooks.test.js`

### Task splits
- **HookImpl** (steps 1-3): Patterns: `patterns.md` P1-P4 | Depends on: none
- **HookWiring** (steps 4-6): Patterns: `guards.md` Settings | Depends on: HookImpl

### File hierarchy
| Level | Component | Depends on |
|-------|-----------|-----------|
| 1 | `hooks/{name}.js` | -- |
| 2 | `settings.json` (registration) | hooks/{name}.js |
| 3 | `hooks/__tests__/hooks.test.js` | hooks/{name}.js |
| 4 | test run | all |

## Recipe: New Slash Command

### Steps
1. Create `commands/mustard/{command-name}/SKILL.md` with trigger, description, procedure, rules → `patterns.md` P11
2. Include `ULTRATHINK` at the end
3. Verify command follows delegation pattern (no direct code implementation)

### Reference module: feature/SKILL.md (complex) | status/SKILL.md (simple)
### Reference files: `commands/mustard/feature/SKILL.md`, `commands/mustard/status/SKILL.md`

### Task splits
- **CommandDef** (steps 1-3): Patterns: `patterns.md` P11 | Depends on: none

### File hierarchy
| Level | Component | Depends on |
|-------|-----------|-----------|
| 1 | `commands/mustard/{name}/SKILL.md` | -- |

## Recipe: New Foundation Skill

### Steps
1. Create `skills/{skill-name}/SKILL.md` with YAML frontmatter → `patterns.md` P12
2. Write pushy description with casual trigger phrases → `guards.md` Skills
3. Add `<!-- mustard:generated -->` after closing `---`
4. Create `skills/{skill-name}/references/examples.md` with real code examples
5. Keep SKILL.md under 500 lines

### Reference module: commit-workflow (simple) | design-craft (with references)
### Reference files: `skills/commit-workflow/SKILL.md`, `skills/design-craft/SKILL.md`

### Task splits
- **SkillDef** (steps 1-5): Patterns: `patterns.md` P12, `guards.md` Skills | Depends on: none

### File hierarchy
| Level | Component | Depends on |
|-------|-----------|-----------|
| 1 | `skills/{name}/SKILL.md` | -- |
| 2 | `skills/{name}/references/examples.md` | SKILL.md |

## Recipe: New Sync Script

### Steps
1. Create `scripts/{script-name}.js` with shebang and JSDoc header
2. Use only Node.js built-ins (fs, path, child_process, crypto)
3. Read ROOT from `path.resolve(__dirname, '..', '..')` → `patterns.md` P8
4. Handle missing files/dirs gracefully with try/catch
5. Output JSON to stdout for consumption by other tools
6. Test manually: `node scripts/{script-name}.js`

### Reference module: sync-detect.js (complex) | sync-registry.js (medium)
### Reference files: `scripts/sync-detect.js`, `scripts/sync-registry.js`

### Task splits
- **ScriptImpl** (steps 1-5): Patterns: `patterns.md` P7, P8 | Depends on: none
- **ScriptTest** (step 6): Depends on: ScriptImpl

### File hierarchy
| Level | Component | Depends on |
|-------|-----------|-----------|
| 1 | `scripts/{name}.js` | -- |
| 2 | manual test | scripts/{name}.js |
