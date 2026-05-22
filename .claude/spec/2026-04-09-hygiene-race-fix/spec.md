# Enhancement: hygiene-race-fix
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Refactor do cleanup em `spec-hygiene.js` para garantir idempotência: cada `fs.unlinkSync` dos arquivos de state em seu próprio `try/catch`. Documentar ordem (rename atomic PRIMEIRO, cleanup best-effort DEPOIS) com comentário explicando recovery semantics. Zero mudança funcional — só robustez a falhas parciais.

## Why
Re-auditoria detectou race window entre `fs.renameSync(specDir)` e `fs.unlinkSync(stateFile)`. Janela é estreita (requer 2 sessões Claude simultâneas), mas o padrão correto é 2-phase commit: move crítico primeiro, cleanup per-file independente depois. Cada cleanup em try/catch separado garante que falha em um não interrompe os outros.

## Boundaries
- `templates/hooks/spec-hygiene.js`
- `.claude/hooks/spec-hygiene.js` (mirror)

## Checklist
### templates-impl Agent
- [x] Ler `templates/hooks/spec-hygiene.js` — identificar o bloco de move+cleanup
- [x] Refactor: rename atomic primeiro (fora do cleanup try/catch), depois loop sobre [stateFile, diffFile] com try/catch independente por arquivo
- [x] Adicionar comentário acima do bloco: `// Phase 1 (critical): atomic rename. Phase 2 (best-effort): orphan state cleanup — each in own try/catch for idempotence.`
- [x] Mirror para `.claude/hooks/spec-hygiene.js`
- [x] Build: `rtk npm run build` → PASS
- [x] Hook tests: `rtk bun test templates/hooks/__tests__/hooks.test.js` → 26/26
- [ ] Smoke test manual (opcional): criar spec fake completed+all-[x], rodar hook, confirmar move + cleanup OK

## Files (~2)
- `templates/hooks/spec-hygiene.js` (modify)
- `.claude/hooks/spec-hygiene.js` (mirror)

## Acceptance
- Rename é atomic e precede qualquer delete
- Cada delete envolto em try/catch individual (idempotente)
- Comentário explicativo presente
- Fail-open preservado (outer try/catch intacto)
- Hook tests 26/26 pass
- Build limpo

## Guards
- NÃO alterar classificação de specs (completed/implementing/silent)
- NÃO alterar guard de `## Concerns BLOCKED`
- NÃO remover fail-open wrapper externo
- Reusar logger/stderr existente — não introduzir console.log novo

## Result
- `templates/hooks/spec-hygiene.js:48-68` — Phase 1/2 two-phase commit refactor: `fs.renameSync` extracted before cleanup block; `stateJson`/`stateDiff` replaced by `stateFile`/`diffFile` iterated via `for...of [stateFile, diffFile]` with individual try/catch per file
- `.claude/hooks/spec-hygiene.js` — mirrored identical changes
- `npm run build`: PASS (tsc clean)
- `bun test`: 26/26 pass
