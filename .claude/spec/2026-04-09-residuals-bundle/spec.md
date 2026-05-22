# Enhancement: residuals-bundle
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-04-09T12:00:00Z

## Summary
Bundle de 4 residual inefficiency fixes, cada um pequeno (~5-15 linhas de código), independentes, arquivos disjuntos. Bundled porque review overhead dominaria se fossem 4 specs separados.

1. **memory-file-rotation**: `session-knowledge-inc.js` rotaciona arquivos >100KB → `.1`
2. **pre-compact-state-validation**: `pre-compact.js` lê `.claude/.pipeline-states/` para identificar spec ativa antes de compactar
3. **sync-registry-in-close**: `complete/SKILL.md` invoca `sync-registry.js` se schema files foram tocadas
4. **context-budget-timeout-chunked**: `context-budget.js` startup usa chunked reads se >50 .md files

## Why
Re-audit listou como residuais de baixa prioridade. Cada um é low-risk, high-simplicity, tiny scope.

## Boundaries
- `templates/hooks/session-knowledge-inc.js` (se existir; senão localizar o hook de knowledge)
- `templates/hooks/pre-compact.js`
- `templates/hooks/context-budget.js`
- `templates/commands/mustard/complete/SKILL.md`
- 4 mirrors em `.claude/`

## Checklist

### Sub-task 1: memory-file-rotation
- [x] Ler hook de knowledge (grep por `session-knowledge` em `templates/hooks/`)
- [x] Antes de cada write: se target file `fs.statSync().size > 100 * 1024` (100KB), renomear para `.1` e começar novo
- [x] Cap total de arquivos rotacionados: 1 (só `.1`, sem cadeia)
- [x] Fail-silently se rotação falha

### Sub-task 2: pre-compact-state-validation
- [x] Ler `templates/hooks/pre-compact.js`
- [x] Antes de compactar: ler `.claude/.pipeline-states/*.json`, filtrar `status: "active"` ou `"implementing"`
- [x] Se 0 active: skip compact (nada pra validar, noop)
- [x] Se 1 active: proceder normal
- [x] Se ≥2 active: log warning stderr, pick most recent `checkpoint` field, proceder
- [x] Fail-open

### Sub-task 3: sync-registry-in-close
- [x] Ler `templates/commands/mustard/complete/SKILL.md` seção CLOSE phase
- [x] Adicionar step condicional: "If the spec's `## Files` list touched any file matching `*.schema.ts`, `*.entity.ts`, `*.prisma`, `*DbContext*.cs`, or `schema.rs`, invoke `rtk node .claude/scripts/sync-registry.js` before marking spec closed. This refreshes `entity-registry.json`."
- [x] Documentar como não-bloqueante: "If sync-registry fails, log warning and proceed with close."

### Sub-task 4: context-budget-timeout-chunked
- [x] Ler `templates/hooks/context-budget.js` startup advisory section
- [x] Se `fs.readdirSync('.claude', {recursive: true})` retorna >50 arquivos `.md`, processar em chunks de 25 (acumulando size)
- [x] Total size final usa mesmo threshold advisory atual
- [x] Fail-open mantido

### Finalização
- [x] Mirror todos os 4 arquivos modificados para `.claude/`
- [x] Build: `rtk npm run build` → PASS
- [x] Hook tests: `rtk bun test templates/hooks/__tests__/hooks.test.js` → 26/26 (sem regressão)
- [x] Smoke test manual de cada sub-task (brief, não precisa ser automated test)

## Files (~8)
- `templates/hooks/session-knowledge-inc.js` (ou equivalente, modify)
- `templates/hooks/pre-compact.js` (modify)
- `templates/hooks/context-budget.js` (modify)
- `templates/commands/mustard/complete/SKILL.md` (modify)
- 4 mirrors em `.claude/`

## Acceptance
- 4 sub-tasks todos `[x]`
- Build PASS
- Hook tests 26/26 sem regressão
- Cada sub-task tem evidence em `## Result` (file:line)
- Todos fail-open

## Guards
- NÃO quebrar hooks existentes (fail-open absoluto)
- Built-ins only em todos
- Sub-task 4 preserva startup advisory behavior
- Sub-task 3 é advisory em complete.md, não blocking
- Se sub-task encontra state estruturalmente diferente do esperado, reportar BLOCKED em vez de forçar

## Waves
- Sub-tasks 1-4 são independentes, podem ser feitas em qualquer ordem
- Finalização (mirror + build + tests) depois de todas as 4

## Result

### Sub-task 1: memory-file-rotation
- `templates/hooks/session-knowledge-inc.js`:131-138 — `writeSeenFile` now checks `statSync().size > 100*1024` before write; if true, `renameSync(file, file+'.1')` then writes fresh. Inner try/catch is fail-open.

### Sub-task 2: pre-compact-state-validation
- `templates/hooks/pre-compact.js`:29-68 — Pipeline state validation block inserted at start of handler. Reads `.pipeline-states/*.json`, filters `active|implementing`. If 0 → exit 0 (noop). If ≥2 → stderr warning + sort by most recent checkpoint/createdAt. Outer try/catch fail-open.

### Sub-task 3: sync-registry-in-close
- `templates/commands/mustard/complete/SKILL.md`:57 — Added conditional schema-aware refresh step under Entity Registry action. Non-blocking (`rtk node .claude/scripts/sync-registry.js`; log warning on failure and proceed).

### Sub-task 4: context-budget-timeout-chunked
- `templates/hooks/context-budget.js`:134-155 — Advisory section now checks `uniquePaths.length > 50`; if true, processes in chunks of 25 accumulating `totalBytes`. Preserves all 3-mode branching (observe/warn/strict) and existing threshold logic.
