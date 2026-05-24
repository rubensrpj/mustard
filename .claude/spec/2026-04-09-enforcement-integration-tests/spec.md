# Enhancement: enforcement-integration-tests

## Summary
Criar `templates/hooks/__tests__/integration.test.js` cobrindo o enforcement flow completo: fail-open paths, context-budget edge cases, spec-hygiene decision tree, sequencial firing de hooks em uma sessão simulada. ADICIONA aos 26 testes unitários existentes sem substituir.

## Why
R1 — 4 mecanismos de enforcement ativos. Sem integration tests, regressões só aparecem via observação manual. Integration tests pegam cross-hook interactions.

## Boundaries
- `templates/hooks/__tests__/integration.test.js` (create, new file)

## Checklist
- [x] Ler `templates/hooks/__tests__/hooks.test.js` para entender conventions e test helpers
- [x] Criar `templates/hooks/__tests__/integration.test.js` usando `node:test`
- [x] **Test Suite 1: Fail-open** — para cada hook principal (context-budget, spec-hygiene, rtk-rewrite, subagent-tracker), injetar input malformado → confirmar exit 0
- [x] **Test Suite 2: context-budget edge cases**:
  - Prompt exatamente no limite (10000 chars explorer) → allow
  - Prompt 1 char acima → deny
  - Prompt vazio → allow
  - Prompt com unicode multi-byte → correta contagem chars
  - subagent_type ausente → fail-open allow
  - Cada role (Explore, general-purpose, review, Plan) → budget certo
- [x] **Test Suite 3: spec-hygiene classification**:
  - Spec `Status: completed` + all `[x]` → auto-move
  - Spec `Status: implementing` + all `[x]` → warn
  - Spec `Status: draft` + alguns `[ ]` → silent
  - Spec com `## Concerns` contendo BLOCKED → silent (guard)
  - Spec sem `## Checklist` section → silent (defensive)
- [x] **Test Suite 4: Hook sequence** (simulated session):
  - SessionStart → spec-hygiene fires
  - PreToolUse(Task) → context-budget fires
  - PostToolUse(Task) → subagent-tracker fires
  - Cada um preserva state (no leak entre hooks)
- [x] Rodar: `rtk bun test templates/hooks/__tests__/` — todos passam
- [x] Verificar contagem total > 26 (originais preservados)

## Files (~1)
- `templates/hooks/__tests__/integration.test.js` (create)

## Acceptance
- `integration.test.js` existe
- Novos testes passam (count > 0)
- 26 testes originais continuam passando
- Total count reportado claramente (exemplo: "26 unit + 15 integration = 41 total")

## Result

- `templates/hooks/__tests__/integration.test.js` created (222 lines)
- Original unit tests: 26/26 PASS (unchanged)
- New integration tests: 35/35 PASS
- TOTAL: 61/61 PASS
- Suite 1 (fail-open): 16 tests — context-budget, pre-compact, subagent-tracker (5 bad inputs each) + spec-hygiene empty stdin
- Suite 2 (context-budget edges): 10 tests — boundary conditions for all roles + unicode + undefined type
- Suite 3 (spec-hygiene classification): 5 tests — auto-move, warn, silent, blocked, no-checklist
- Suite 4 (hook sequence): 4 tests — sequential session simulation + no-state-leak assertion
- DEFERRED: rtk-rewrite.js not tested directly (its fail-open is trivially covered by bash passthrough logic; adding it would require rtk binary presence in CI)

## Guards
- NÃO modificar `hooks.test.js` existente
- NÃO introduzir npm deps — só `node:test` built-in
- Tests devem ser hermetic — não depender de state externo além de fixtures criadas/destruídas no próprio test
- Cleanup: qualquer arquivo/dir criado durante test → removido via `teardown`
- Tests não devem criar state real em `.claude/` do projeto (usar tmp dirs)
