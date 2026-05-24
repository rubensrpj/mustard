# Enhancement: budget-observability

## Summary
Adicionar 3 modos ao `context-budget.js`: `strict` (atual, hard-block), `warn` (log + allow), `observe` (log tamanho real a `.claude/.metrics/budget-observations.jsonl`, zero block). Mode via env var `CONTEXT_BUDGET_MODE`. Default = `strict` (zero mudança comportamental sem opt-in explícito). Permite coletar dados reais para tuning futuro do threshold.

## Why
Thresholds atuais (10K/20K/12K chars) vieram de `tokens × 4`, não medidos. Risco de falsos positivos em prompts legítimos. Observe mode coleta sem bloquear.

## Boundaries
- `templates/hooks/context-budget.js` (extend)
- `.claude/hooks/context-budget.js` (mirror)

## Checklist
- [x] Ler `templates/hooks/context-budget.js` atual (já tem PreToolUse(Task) + startup advisory)
- [x] Adicionar detecção de modo: `const MODE = process.env.CONTEXT_BUDGET_MODE || 'strict';`
- [x] Branch `observe`: escrever entry JSONL em `.claude/.metrics/budget-observations.jsonl` com `{ts, role, actual_chars, limit, would_block}` — exit 0 allow
- [x] Branch `warn`: stderr log + exit 0 allow
- [x] Branch `strict`: comportamento atual preservado
- [x] Criar `.claude/.metrics/` on-demand (mkdirSync recursive)
- [x] Preservar startup advisory check
- [x] Preservar outer try/catch (fail-open)
- [x] Mirror → `.claude/hooks/context-budget.js`
- [x] Build + hook tests 26/26

## Result
Implemented 3-mode branching in `templates/hooks/context-budget.js` (lines 26-128). `strict` default preserved. `warn` logs to stderr + allows. `observe` appends JSONL to `.claude/.metrics/budget-observations.jsonl` + allows. Mirrored to `.claude/hooks/context-budget.js`. Build PASS, tests 26/26.

## Files (~2)
- `templates/hooks/context-budget.js` (modify)
- `.claude/hooks/context-budget.js` (mirror)

## Acceptance
- 3 modos funcionam; default `strict` preservado
- `observe` cria arquivo JSONL sem bloquear
- `warn` loga sem bloquear
- Build + tests 26/26

## Guards
- Default MUST be `strict` — zero mudança sem opt-in
- `.claude/.metrics/` dir criado on-demand, não no startup
- Fail-open absoluto — erro no hook → exit 0
- NO npm deps — built-ins only (fs, path)
