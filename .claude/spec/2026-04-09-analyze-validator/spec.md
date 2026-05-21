# Enhancement: analyze-validator
### Status: completed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Criar `templates/scripts/analyze-validation.js` (~60 linhas, built-ins only) que lê um spec.md no final do ANALYZE phase e retorna JSON `{ok, issues}` com 3 validações WARN-level (nunca bloqueantes):
1. **Layer coverage** — se spec menciona agent "Backend/Frontend/Database" mas `## Files` não tem extensão correspondente → WARN
2. **File refs resolvable** — cada entrada em `## Files` existe OU é marcada `(create)` → WARN
3. **Task decomposition sane** — cada `### {Agent} Agent` tem 2-10 checklist items → WARN

Output é sempre JSON. WARNs são anotadas no spec em `## Concerns` pela pipeline, ANALYZE continua. Zero blocking. Fail-safe absoluto: erro interno → exit 1 + JSON `{ok:false, issues:[{severity:"ERROR", type:"validator-crash", ...}]}` — orchestrator decide.

Integrar no `/mustard:feature` no final do ANALYZE phase, antes de passar para PLAN: invocar o script, anotar WARNs em `## Concerns`, seguir.

**Reescopo do bundle original**: removido entity-registry check (registry vazio, causaria false positives — será corrigido em spec separado futuro), removido WARN ambiguous keywords (redundante com review humano). Só validações crisp e determinísticas.

## Why
Spec `reference_mustard_token_efficiency.md` mostra que ANALYZE incompleto é causa raiz de fix loops pós-review (170K+266K tokens extras por loop). Um validator pequeno, não-bloqueante, que surface-a os problemas óbvios antes do PLAN reduz a chance do spec ir meio torto para EXECUTE. Sendo WARN-only, não introduz false-positive friction.

## Boundaries
- `templates/scripts/analyze-validation.js` (create)
- `templates/commands/mustard/feature/SKILL.md` (modify — 1 invocation point)
- `.claude/scripts/analyze-validation.js` (mirror)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)

## Checklist
### templates-impl Agent
- [x] Ler `templates/commands/mustard/feature/SKILL.md` seção ANALYZE para entender onde termina (antes de PLAN)
- [x] Criar `templates/scripts/analyze-validation.js`:
  - CLI: `node analyze-validation.js --spec <path>` OU stdin JSON `{specPath}`
  - Lê spec.md, parseia header (`Status:`, `Scope:`), seções (`## Files`, `## Checklist`/`### Agent`)
  - Validação 1 — Layer coverage: procura headers `### (Backend|Frontend|Database|Mobile) Agent`, depois verifica se `## Files` tem extensões coerentes. Mapa: Backend→{.ts,.cs,.py,.go,.rs}, Frontend→{.tsx,.jsx,.vue,.svelte}, Database→{.sql,schema files}. Ausência → WARN
  - Validação 2 — File refs: regex `\`([\w./-]+\.\w+)\`` extrai paths; para cada, `fs.existsSync()` OU a linha contém `(create)` → pass. Caso contrário → WARN
  - Validação 3 — Task count: conta `- [ ]` e `- [x]` por agent section; <2 ou >10 → WARN
  - Output: JSON `{ok: <boolean>, issues: [{severity:"WARN", type, message, file?}]}`
  - Fail-safe outer try/catch: erro → exit 1 + JSON error shape
  - Built-ins only: fs, path, process
- [x] Em `templates/commands/mustard/feature/SKILL.md`, no FINAL do ANALYZE phase (antes da transição para PLAN), adicionar step:
  ```
  At end of ANALYZE phase, run `rtk node .claude/scripts/analyze-validation.js --spec .claude/spec/active/{name}/spec.md`. 
  If output `ok: false`, append each `issues[]` entry to the spec under `## Concerns` (non-blocking). Continue to PLAN regardless.
  ```
- [x] Mirror script → `.claude/scripts/analyze-validation.js`
- [x] Mirror feature.md → `.claude/commands/mustard/feature/SKILL.md`
- [x] Smoke test: criar spec sintético com problemas conhecidos (Frontend agent sem .tsx em Files), rodar validator, confirmar WARN returned; depois spec limpo, confirmar `{ok:true, issues:[]}`
- [x] Build: `rtk npm run build` → PASS
- [x] Hook tests: `rtk bun test templates/hooks/__tests__/hooks.test.js` → 26/26

## Files (~4)
- `templates/scripts/analyze-validation.js` (create)
- `templates/commands/mustard/feature/SKILL.md` (modify)
- `.claude/scripts/analyze-validation.js` (mirror)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)

## Acceptance
- Script existe, built-ins only, ~60 linhas
- 3 validações WARN-only implementadas
- Output é sempre JSON (sucesso ou erro)
- Fail-safe: erro interno → exit 1 + JSON error
- Invocação integrada em feature.md ANALYZE phase
- Espelhado em `.claude/`
- Smoke test passa
- Build limpo
- Hook tests 26/26

## Guards
- NUNCA bloquear ANALYZE → PLAN (só anotar em Concerns)
- NUNCA gerar ERROR-level de próprio (só WARN). ERROR reservado para crash interno.
- NUNCA rodar em Light scope (desnecessário — Light já é minimal)
- NÃO introduzir npm deps
- NÃO criar seção `## Concerns` no spec se ela já existir (append, não replace)
- Script é idempotente: rodar 2x no mesmo spec → mesmo resultado

## Result
- `templates/scripts/analyze-validation.js`: 80 lines, built-ins only (fs, path, process)
- Integration point: `feature/SKILL.md` after "Compact Advisory" block, before `### PLAN Phase` (~line 110)
- Clean spec smoke test: `{ok: true, issues: []}`
- Broken spec smoke test: `{ok: false, issues: [layer-gap, missing-file×2, task-count]}` — all 3 validations fired
- Build: PASS (tsc clean)
- Hook tests: 26/26
