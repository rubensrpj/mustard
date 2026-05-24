# Enhancement: budget-gate-enforcement

## Summary
Extender `templates/hooks/context-budget.js` para bloquear (não apenas avisar) dispatches de Task que excedam budgets por role: explorer ≤2500 chars no prompt, impl ≤5000, review ≤3000. Hoje o hook é apenas advisory (verifica `.claude/` markdown footprint). Este enhancement adiciona uma segunda verificação: o tamanho do `prompt` do Task sendo despachado.

## Why
`pipeline-config.md` declara os budgets textualmente (impl ≤5K, explorer ≤2.5K, review ≤3K) mas nada enforcça. Sessões recentes mostraram agents recebendo prompts gordos por acidente, gastando tokens de ANALYZE extra. Hook PreToolUse(Task) é o único ponto onde dá pra medir antes do dispatch. Blocking decisions aumentam a disciplina sem exigir mudanças nos SKILL.md.

## Boundaries
- `templates/hooks/context-budget.js` — main file (extend)
- `templates/settings.json` — may need to add PreToolUse(Task) matcher if hook hoje é startup only
- `.claude/hooks/context-budget.js` — mirror
- `.claude/settings.json` — mirror
- `.claude/pipeline-config.md` — reference budgets (read-only)

## Checklist
### templates-impl Agent
- [x] Ler `templates/hooks/context-budget.js` atual — entender API (stdin JSON, event type, current checks)
- [x] Ler `templates/settings.json` — confirmar como `context-budget.js` é registrado hoje (provavelmente startup advisory)
- [x] Ler `.claude/pipeline-config.md` — buscar os budgets declarados (palavras-chave: "≤5K", "≤2.5K", "≤3K", "budget")
- [x] Desenhar extensão: PreToolUse(Task) branch que lê `input.prompt` e infere role a partir de `input.subagent_type` + `input.description`/`input.prompt` keywords (explorer|impl|review)
- [x] Implementar classificação de role: `Explore` → explorer, `Plan` → advisory only, `general-purpose` com `review` no description → review, senão → impl
- [x] Medir `prompt.length` (chars, não tokens — 1 token ≈ 3-4 chars, budget em chars seria ~10K/5K/6K; OU converter budgets para chars assumindo 4 chars/token: 20K/10K/12K). **Decisão: budgets textuais são em tokens, usar 4 chars/token como aproximação conservadora**: explorer ≤10000 chars, impl ≤20000 chars, review ≤12000 chars
- [x] Retornar `permissionDecision: "deny"` com reason clara se exceder; retornar `permissionDecision: "allow"` caso contrário
- [x] Fail-open: qualquer erro interno → exit 0 (não bloquear por bug do hook)
- [x] Adicionar PreToolUse(Task) matcher em `templates/settings.json` se não existir; se já existir com outro hook, adicionar `context-budget.js` na lista
- [x] Espelhar para `.claude/hooks/context-budget.js` e `.claude/settings.json`
- [x] Testar manualmente: escrever payload stdin JSON (prompt grande) e rodar `node templates/hooks/context-budget.js < payload.json`
- [x] Build/type-check: `rtk npm run build`
- [x] Rodar testes existentes de hooks se houver: `rtk bun test hooks/__tests__/hooks.test.js`

## Files (~4-5)
- `templates/hooks/context-budget.js` (modify — main extension)
- `templates/settings.json` (modify if Task matcher not registered)
- `.claude/hooks/context-budget.js` (mirror)
- `.claude/settings.json` (mirror)

## Acceptance
- Hook dispara em PreToolUse(Task), classifica role, compara prompt.length vs budget
- Retorna deny quando excede, com reason informativa (role, limit, actual)
- Fail-open preservado (exit 0 em erro interno)
- Template e .claude em sync
- Build limpo
- Memoradum em `pipeline-config.md` OU comentário no hook sobre como ajustar budgets (não hardcode sem explicação)

## Result
- Hook extended with hard-block enforcement on PreToolUse(Task); advisory logic preserved intact
- Role classification: `getBudget()` function routes by `subagent_type` + `description`
- Budget constants at top of file with explicit token→char conversion comments
- `settings.json`: Task matcher already registered — no changes needed (context-budget.js was already in the hook list alongside subagent-tracker.js)
- Mirrored: `templates/hooks/context-budget.js` → `.claude/hooks/context-budget.js`
- Manual tests: deny (Explore 10001 chars), allow (Explore 9999 chars), deny (review 12001 chars), pass-through (Plan 50000 chars)
- Build: tsc PASS (no output = success)
- Hook tests: 26/26 PASS

## Guards
- NÃO quebrar o advisory startup check existente
- NÃO bloquear matcher `Explore` subagent (delegado puro de navegação pode precisar prompt maior pra contexto)
- NÃO contar system prompt, só o `input.prompt` que é o corpo do Task
