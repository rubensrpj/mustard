# Wave 2a — Refactor Low-Risk Files via Progressive Disclosure

> Reference: `../wave-plan.md`

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Wave: 2a/3
### Checkpoint: 2026-04-23
### Depends on: wave-1-size-gate-infra (CLOSED)

## Summary

Aplicar progressive disclosure nos 3 arquivos de baixo risco (`templates/CLAUDE.md`, `git/SKILL.md`, `scan/SKILL.md`). Preservar 100% do conteúdo — seções autônomas vão para `references/*.md` próprios. Validar com gate da Wave 1: target ≤200 linhas no body, nenhuma referência quebrada. **Fora de escopo:** `feature/SKILL.md` (vai para Wave 2b com approval isolado).

## Entity Info

| Entity | Layer | Op |
|--------|-------|----|
| templates/CLAUDE.md | docs | REFACTOR (206 → ≤200) |
| templates/commands/mustard/git/SKILL.md | command | REFACTOR (588 → ≤200) |
| templates/commands/mustard/scan/SKILL.md | command | REFACTOR (450 → ≤200) |
| references/*.md (5 novos) | docs | NEW |

## Files (~10)

- `templates/CLAUDE.md` (modify — extract Cost Optimization Hooks + Enforcement Hooks + Shared Memory tables)
- `templates/pipeline-config.md` OR equivalente (RECEIVE tabelas extraídas — confirmar path antes)
- `templates/commands/mustard/git/SKILL.md` (modify — keep ≤200 lines)
- `templates/commands/mustard/git/references/git-flow.md` (create)
- `templates/commands/mustard/git/references/submodule-rules.md` (create)
- `templates/commands/mustard/git/references/merge-protocol.md` (create)
- `templates/commands/mustard/scan/SKILL.md` (modify — keep ≤200 lines)
- `templates/commands/mustard/scan/references/scan-protocol.md` (create)
- `templates/commands/mustard/scan/references/evidence-rules.md` (create)
- `.claude/commands/mustard/{git,scan}/SKILL.md` + references/ (MIRROR)

## Boundaries

- `templates/CLAUDE.md` — exact file
- `templates/commands/mustard/git/**` — directory scope
- `templates/commands/mustard/scan/**` — directory scope
- `templates/pipeline-config.md` (ou onde as tabelas extraídas forem) — read+modify
- `.claude/commands/mustard/{git,scan}/**` — mirror only (final step)
- **Out of bounds:** `templates/commands/mustard/feature/**` (Wave 2b), approve/, complete/, resume/, bugfix/, qa/, knowledge/, scan-format/, task/

## Checklist

### General Agent (Wave 2a.1 — extractions, parallel-safe per file)

> **Layout note (decided mid-execution):** refs moved to `templates/refs/{cmd}/` instead of `commands/mustard/{cmd}/references/`. Claude Code auto-registers any subdirectory of `commands/` as a sub-command (underscore prefix does not skip discovery), so refs must live OUTSIDE `commands/`. Mirror at `.claude/refs/{cmd}/`.

- [x] **git/SKILL.md split (588 → 69):**
  - Read full original.
  - Extract autonomous sections to:
    - `templates/refs/git/git-flow.md` (112 lines) → tabelas de comandos, fluxo dev_rubens/dev/main, config `mustard.json`
    - `templates/refs/git/submodule-rules.md` (107 lines) → detecção/escopo de submódulos, rules de commit por subproject
    - `templates/refs/git/merge-protocol.md` (277 lines) → ff-only protocol, `/git merge main` flow, tratamento de conflitos
  - SKILL.md final: YAML frontmatter + Trigger + Description + 1-line por ação com pointer `→ refs/git/X.md` quando o detalhe importa.
- [x] **scan/SKILL.md split (450 → 57):**
  - Read full original (inclui mudanças unstaged: EVIDENCE RULE + Validate Skills step).
  - Extract:
    - `templates/refs/scan/scan-protocol.md` (367 lines) → execution rule, agent dispatch sequence, `--force` handling, read-before-write, EXECUTION RULE "NO CONFIRMATION PROMPTS"
    - `templates/refs/scan/evidence-rules.md` (120 lines) → EVIDENCE RULE 1-5 + seção Validate Skills
  - SKILL.md final: trigger + fases resumidas (1 linha cada) + refs.
- [x] **templates/CLAUDE.md compactação (206 → 123):**
  - Read full.
  - Tabelas `## Cost Optimization Hooks` + `## Enforcement Hooks` + `## Shared Memory Architecture` (incluindo subseções) movidas para `templates/pipeline-config.md`.
  - CLAUDE.md final mantém: Role + Intent Routing + Pipeline Phases + QA Phase + Context Loading + Stack + Commands + Guards + Scan References + Recommended Skills + Token Economy (1-line summary) + Full Reference link.

### General Agent (Wave 2a.2 — sync to .claude/, sequential, after 2a.1)

- [x] Copy `templates/commands/mustard/{git,scan}/SKILL.md` → `.claude/commands/mustard/{git,scan}/SKILL.md`
- [x] Copy `templates/refs/{git,scan}/` → `.claude/refs/{git,scan}/` (path adjusted per Layout note)
- [x] Verify each `.claude/` file ≤200 lines (git=69, scan=57).

### General Agent (Wave 2a.3 — validation, sequential)

- [x] Run `node templates/scripts/skill-validate.js --lines --json` → no `tier === "block"` for git/scan skills.
- [x] **Content preservation grep** (each must find match in SKILL.md OR `refs/{cmd}/`):
  - git: `mustard.json`, `ff-only`, `submodule`, `dev_rubens`, `merge`
  - scan: `EVIDENCE RULE`, `--factual`, `<!-- mustard:generated -->`, `cluster`, `NO CONFIRMATION PROMPTS`
  - CLAUDE.md ecosystem: `MUSTARD_BASH_REDIRECT_MODE`, `duplication-check`, `harness/events.jsonl`, `knowledge.json` (somewhere in templates/)

## Acceptance Criteria

- [x] AC-1: 3 target files ≤200 lines — Command: `node -e "const fs=require('fs');const files=['templates/CLAUDE.md','templates/commands/mustard/git/SKILL.md','templates/commands/mustard/scan/SKILL.md'];for(const f of files){const n=fs.readFileSync(f,'utf8').split('\n').length;if(n>200){console.error('FAIL:',f,'=',n);process.exit(1)}}"`
- [x] AC-2: No `block` tier for git/scan skills — Command: `node templates/scripts/skill-validate.js --lines --json | node -e "const j=JSON.parse(require('fs').readFileSync(0,'utf8'));const bad=j.results.filter(r=>r.tier==='block' && (r.file.includes('/git/')||r.file.includes('/scan/')));if(bad.length)process.exit(1)"`
- [x] AC-3: Key tokens preserved — Command: `node -e "const fs=require('fs'),path=require('path');function walk(d){const o=[];for(const e of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,e.name);if(e.isDirectory())o.push(...walk(p));else if(p.endsWith('.md'))o.push(p)}return o}const files=[...walk('templates/commands/mustard/git'),...walk('templates/refs/git'),...walk('templates/commands/mustard/scan'),...walk('templates/refs/scan'),'templates/CLAUDE.md','templates/pipeline-config.md'];const all=files.map(f=>fs.readFileSync(f,'utf8')).join('\n');const tokens=['mustard.json','ff-only','EVIDENCE RULE','MUSTARD_BASH_REDIRECT_MODE'];const missing=tokens.filter(t=>!all.includes(t));if(missing.length){console.error('MISSING:',missing);process.exit(1)}"`
- [x] AC-4: All new refs/ files non-empty — Command: `node -e "const fs=require('fs'),path=require('path');const dirs=['templates/refs/git','templates/refs/scan'];for(const d of dirs){const files=fs.readdirSync(d).filter(f=>f.endsWith('.md'));if(!files.length){console.error('EMPTY DIR:',d);process.exit(1)}for(const f of files){if(!fs.statSync(path.join(d,f)).size){console.error('EMPTY:',f);process.exit(1)}}}"`
- [x] AC-5: Mirror to .claude/ — Command: `node -e "const fs=require('fs');for(const d of ['git','scan']){const a=fs.readFileSync('templates/commands/mustard/'+d+'/SKILL.md','utf8');const b=fs.readFileSync('.claude/commands/mustard/'+d+'/SKILL.md','utf8');if(a!==b){console.error('DIFF:',d);process.exit(1)}}"`
- [x] AC-6: Hook tests still pass — Command: `node -e "const{execSync}=require('child_process');try{execSync('bun test templates/hooks/__tests__/hooks.test.js templates/hooks/__tests__/size-gates.test.js',{stdio:'pipe',timeout:120000})}catch(e){process.exit(1)}"`

## Dependencies

- Wave 1 complete (size-gate hooks + `--lines` flag active)

## Concerns

- Path do pipeline-config destino: confirmar se é `templates/pipeline-config.md`, `templates/.claude/pipeline-config.md`, ou criar novo. Read pré-implementação.
- `.claude/skills/skill-creator/SKILL.md` (485 linhas) fora de escopo (externo, não Mustard-owned).
