# Enhancement: budget-gate-doc-fix
### Status: completed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Fix documentation drift: o spec anterior de `budget-gate-enforcement` declarava "não bloquear Explore" nos Guards, mas o código implementa hard-block em ~10K chars (o que está correto conforme `pipeline-config.md`). Este patch apenas clarifica semântica — zero mudança comportamental. O budget aplica ao `input.prompt` (o briefing do Task), NÃO ao contexto que o explorer coleta internamente via Grep/Read.

## Why
Re-auditoria detectou confusão conceitual no guard textual. Código está correto, documentação é que estava errada. Confusão futura levaria alguém a "afrouxar" o budget achando que estava travando exploração, quando na verdade só trava prompts oversized.

## Boundaries
- `templates/hooks/context-budget.js` (comment-only)
- `.claude/hooks/context-budget.js` (mirror)
- `.claude/pipeline-config.md` (doc clarification, se a seção budgets existir)

## Checklist
### templates-impl Agent
- [x] Adicionar comentário no topo de `templates/hooks/context-budget.js` explicando: `// BUDGETS apply to input.prompt (the Task briefing), not to context agents gather internally.`
- [x] Se `.claude/pipeline-config.md` tem seção de budgets, adicionar linha clarificando "measurement scope = Task input.prompt only"
- [x] Mirror comment change para `.claude/hooks/context-budget.js`
- [x] Build: `rtk npm run build` → PASS
- [x] Hook tests: `rtk bun test templates/hooks/__tests__/hooks.test.js` → 26/26

## Files (~3)
- `templates/hooks/context-budget.js` (comment)
- `.claude/hooks/context-budget.js` (mirror)
- `.claude/pipeline-config.md` (doc, se aplicável)

## Acceptance
- Comentário presente no topo do hook
- Nenhuma mudança de lógica/comportamento
- Tests still 26/26
- Build limpo

## Result
Files modified:
- `templates/hooks/context-budget.js`: added 3-line "IMPORTANT" comment block clarifying budget scope (lines 7-10)
- `.claude/hooks/context-budget.js`: mirrored same comment block
- `.claude/pipeline-config.md`: added clarifying line after Token Budget table

Build: PASS (tsc clean)
Tests: 26/26 pass
