# Wave 2b — Refactor feature/SKILL.md (High-Risk, Approval-Gated)

> Reference: `../wave-plan.md`

### Status: completed | Phase: CLOSE | Scope: full | Wave: 2b/3
### Checkpoint: 2026-04-23
### Depends on: wave-2a-refactor-low-risk (CLOSED)

## Summary

Refactor crítico do `feature/SKILL.md` (458 linhas) — coração do pipeline. Body ≤200 linhas + 3 `references/*.md`. Também introduz documentação oficial do layout multi-arquivo para specs FUTURAS. Wave isolada com approval próprio — antes de executar, leitura humana do spec é recomendada.

## Entity Info

| Entity | Layer | Op |
|--------|-------|----|
| templates/commands/mustard/feature/SKILL.md | command | REFACTOR (458 → ≤200) |
| feature/references/*.md (3 novos) | docs | NEW |

## Files (~6)

- `templates/commands/mustard/feature/SKILL.md` (modify)
- `templates/commands/mustard/feature/references/spec-hygiene.md` (create)
- `templates/commands/mustard/feature/references/wave-decomposition.md` (create)
- `templates/commands/mustard/feature/references/existence-gate.md` (create)
- `.claude/commands/mustard/feature/SKILL.md` + references/ (MIRROR)

## Boundaries

- `templates/commands/mustard/feature/**` — directory scope
- `.claude/commands/mustard/feature/**` — mirror only (final step)
- **Out of bounds:** outras commands, hooks, scripts, settings.

## Checklist

### General Agent (Wave 2b.1 — extraction)

> **Layout note (decided mid-execution):** refs moved to `templates/refs/feature/` instead of `commands/mustard/feature/references/`. Same reason as wave-2a: Claude Code auto-registers `commands/` subdirs. Mirror at `.claude/refs/feature/`.

- [x] Read full `templates/commands/mustard/feature/SKILL.md` (458 linhas).
- [x] Extract to `templates/refs/feature/spec-hygiene.md` (23 lines):
  - Todo o bloco `### Spec Hygiene (automatic, before ANALYZE)` (passos 1-5: scan, verify completed/cancelled, in-progress handling, no-active path)
- [x] Extract to `templates/refs/feature/wave-decomposition.md` (105 lines):
  - Todo o bloco `#### Wave Decomposition Pre-Check (Full scope only)` (passos 1-9: compute signals, knowledge matches, scope-decompose call, wave-dependency call, wave plan structure, pipeline state for wave plan, present wave plan, user approval options)
  - Inclui definição de `wave-plan.md` template + per-wave `spec.md` template
- [x] Extract to `templates/refs/feature/existence-gate.md` (56 lines):
  - Todo o bloco `### Pre-EXECUTE Existence Gate (Full scope only)` (skip conditions, pre-check, Haiku dispatch prompt, decision table all-no/mixed/all-yes)
- [x] Rewrite `SKILL.md` final com estrutura enxuta:
  - Trigger + Description (≤10 linhas)
  - `## Action` (≤20 linhas) — intro + phase order
  - `### Spec Hygiene` — 1 linha + `→ references/spec-hygiene.md`
  - `### ANALYZE Phase` — inline (passos 1-3 resumidos: auto-sync, diff context, layers table)
  - `#### Scope Detection` — tabela resumida + 1-2 linhas
  - `#### Explore` — Path A / Path B 1 linha cada
  - `#### Compact Advisory` — 2 linhas
  - `### Decomposition Rule` — 2 linhas + `→ references/wave-decomposition.md`
  - `### End of ANALYZE — Validation` — 2 linhas (comando)
  - `### PLAN Phase` — 1 linha de intro + `→ references/wave-decomposition.md` para Full decomposição
  - `#### Full Scope` — inline (spec.md template bulleted, AC section, present-to-user)
  - `#### Light Scope` — inline (compact template)
  - `#### Spec Boundaries` — inline (5 linhas)
  - `### Pre-EXECUTE Existence Gate` — 1 linha + `→ references/existence-gate.md`
  - `### EXECUTE Phase (Light)` — inline resumido
  - `#### Escalation Status Handling` — inline 5 linhas
  - `#### Failure Routing` — inline 5 linhas
  - `### QA Phase (Wave 10)` — inline (já curto no original, ~10 linhas)
  - `## Visual Output` — inline (2 linhas)
  - **`## Spec Layout` (nova seção, ≤20 linhas):**
    ```
    Specs may grow beyond 200 lines. When that happens, apply the same progressive disclosure pattern:
    - Default: `spec.md` único (Light OR Full small).
    - When spec.md > 200 lines: extract autonomous sections to `spec-references/{section}.md` in the SAME directory; SKILL.md body keeps pointers.
    - Hard block at 500 lines (gate `MUSTARD_SPEC_SIZE_MODE=strict`; default `warn`).
    - For Wave plans: `wave-plan.md` + per-wave `spec.md` already follow this pattern.
    ```
  - `## Rules` — inline (já curto, ~15 bullets)
  - End with `ULTRATHINK` marker.
- [x] Verify ≤200 linhas (200 no commit `e2102d6` — drift posterior tratado em wave-2b.4).

### General Agent (Wave 2b.2 — sync to .claude/, sequential)

- [x] Copy `templates/commands/mustard/feature/SKILL.md` → `.claude/commands/mustard/feature/SKILL.md`
- [x] Copy `templates/refs/feature/` → `.claude/refs/feature/` (path adjusted per Layout note)

### General Agent (Wave 2b.3 — validation, sequential)

- [x] Run `node templates/scripts/skill-validate.js --lines --json` → feature skill NOT in `block` tier.
- [x] **Content preservation grep** (each token must appear in SKILL.md OR `refs/feature/`):
  - `Spec Hygiene`, `AskUserQuestion`, `mark completed but has`, `Scan all specs`
  - `Wave Decomposition`, `scope-decompose.js`, `wave-dependency.js`, `wave-plan.md`, `COORDINATE phase`
  - `Pre-EXECUTE Existence Gate`, `all_present`, `Haiku`, `already-implemented`
  - `Acceptance Criteria`, `AC-1`, `runnable command`
  - `Light scope`, `Full scope`, `Extended Light`, `Decomposition Rule`
  - `Escalation Statuses`, `BLOCKED`, `PARTIAL`, `DEFERRED`, `CONCERN`
  - `Failure Routing`, `Transient`, `Resolvable`, `Structural`
  - `entity-registry.json`, `pipeline-config.md`, `recipe-match.js`
- [x] **Dry-run check:** parse novo SKILL.md para verificar que:
  - YAML frontmatter (se existir) + title + trigger section exist
  - Cada `→ refs/feature/X.md` link corresponde a um arquivo real
- [x] Optional: rodar `/mustard:status` para ver se o pipeline-state atual é lido corretamente (não afeta código, só leitura).

## Acceptance Criteria

- [x] AC-1: feature SKILL.md ≤200 linhas — Command: `node -e "if(require('fs').readFileSync('templates/commands/mustard/feature/SKILL.md','utf8').split('\n').length>200)process.exit(1)"`
- [x] AC-2: Todas 3 refs non-empty — Command: `node -e "const fs=require('fs');for(const f of ['spec-hygiene','wave-decomposition','existence-gate']){if(!fs.statSync('templates/refs/feature/'+f+'.md').size)process.exit(1)}"`
- [x] AC-3: Key tokens preserved anywhere under `feature/` — Command: `node -e "const tokens=['Spec Hygiene','Wave Decomposition','Existence Gate','Acceptance Criteria','Extended Light','Escalation Status','Failure Routing','COORDINATE phase','Pre-EXECUTE','all-no','recipe-match.js'];const fs=require('fs');const path=require('path');function walk(d){const out=[];for(const e of fs.readdirSync(d,{withFileTypes:true})){const p=path.join(d,e.name);if(e.isDirectory())out.push(...walk(p));else out.push(p)}return out}const files=[...walk('templates/commands/mustard/feature'),...walk('templates/refs/feature')].filter(f=>f.endsWith('.md'));const all=files.map(f=>fs.readFileSync(f,'utf8')).join('\n');const missing=tokens.filter(t=>!all.includes(t));if(missing.length){console.error('MISSING:',missing);process.exit(1)}"`
- [x] AC-4: Link integrity — every `refs/feature/X.md` referenced in SKILL.md body actually exists — Command: `node -e "const fs=require('fs');const path=require('path');const body=fs.readFileSync('templates/commands/mustard/feature/SKILL.md','utf8');const refs=[...body.matchAll(/refs\/feature\/([a-z0-9-]+\.md)/g)].map(m=>m[1]);const missing=refs.filter(r=>!fs.existsSync(path.join('templates/refs/feature',r)));if(missing.length){console.error('MISSING:',missing);process.exit(1)}"`
- [x] AC-5: Mirror to .claude/ — Command: `node -e "const fs=require('fs');if(fs.readFileSync('templates/commands/mustard/feature/SKILL.md','utf8')!==fs.readFileSync('.claude/commands/mustard/feature/SKILL.md','utf8'))process.exit(1)"`
- [x] AC-6: `## Spec Layout` section present — Command: `node -e "if(!/^## Spec Layout/m.test(require('fs').readFileSync('templates/commands/mustard/feature/SKILL.md','utf8')))process.exit(1)"`
- [x] AC-7: Hook tests still pass — Command: `node -e "const{execSync}=require('child_process');try{execSync('bun test templates/hooks/__tests__/hooks.test.js templates/hooks/__tests__/size-gates.test.js',{stdio:'pipe',timeout:120000})}catch(e){process.exit(1)}"`

## Dependencies

- Wave 2a complete (canonical `SKILL.md + references/` layout already applied to git, scan as live examples)

## Concerns

- **Alto risco:** esta é a spec mais crítica. Recomendo ler o novo `SKILL.md` no final do Wave 2b.1 e antes do Wave 2b.2 (mirror). Se algo soa estranho, reject + retry antes de propagar para `.claude/`.
- `## Spec Layout` é uma adição de instrução, não um extraction. Nova capability: specs futuras podem (e vão) se auto-decompor.
- Considerar feature flag: adicionar env var `MUSTARD_SPEC_AUTO_SPLIT` (default `off` por enquanto) que instrui `/mustard:feature` a gerar `spec-references/` automaticamente quando detecta body > 200 linhas durante PLAN. Pode ser deixado para futuro — este spec só documenta o layout.
