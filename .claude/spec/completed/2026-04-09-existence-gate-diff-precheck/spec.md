# Enhancement: existence-gate-diff-precheck
### Status: completed | Phase: CLOSE | Scope: light
### Checkpoint: 2026-04-09T00:00:00Z

## Summary
Adicionar pre-check de git diff ANTES do dispatch Haiku no Pre-EXECUTE Existence Gate. Se os arquivos do spec não têm mudanças (ou <10 linhas changed), **pular o gate inteiramente**. Cost do pre-check: ~50ms bash call + 0 tokens LLM. Elimina custo fixo de ~2.5K tokens por pipeline Full onde nada mudou desde criação do spec.

## Why
R4 — Existence gate custa ~2.5K tokens mesmo quando spec foi recém criado. Git diff pre-check detecta esse caso "nothing to verify" de graça.

## Boundaries
- `templates/commands/mustard/feature/SKILL.md` (modify — add pre-check before gate dispatch)
- `templates/commands/mustard/resume/SKILL.md` (modify — same pre-check)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/resume/SKILL.md` (mirror)

## Checklist
- [x] Localizar seção "Pre-EXECUTE Existence Gate" em `templates/commands/mustard/feature/SKILL.md`
- [x] Inserir IMEDIATAMENTE ANTES do dispatch Haiku (antes do `Task({...})` block) um bloco pre-check:
  ```markdown
  **Pre-check (free, no LLM)**: Before dispatching Haiku, run:
  \`\`\`bash
  rtk git diff --stat HEAD -- <files-from-## Files-section>
  \`\`\`
  Skip rules:
  - Empty output → skip gate, proceed to EXECUTE (nothing changed to verify)
  - <10 total insertions/deletions → skip gate, proceed to EXECUTE (trivial changes, not worth verifying)
  - ≥10 insertions/deletions → proceed with Haiku dispatch
  ```
- [x] Localizar step `12b. **Pre-EXECUTE Existence Gate**` em `templates/commands/mustard/resume/SKILL.md`
- [x] Adicionar o mesmo pre-check block antes do dispatch (ou referência DRY ao feature.md)
- [x] Mirror para `.claude/commands/mustard/{feature,resume}/SKILL.md`
- [x] Build + hook tests 26/26
- [x] Walkthrough mental: 3 cenários (empty diff, small diff <10, big diff ≥10)

## Files (~4)
- `templates/commands/mustard/feature/SKILL.md`
- `templates/commands/mustard/resume/SKILL.md`
- `.claude/commands/mustard/feature/SKILL.md`
- `.claude/commands/mustard/resume/SKILL.md`

## Acceptance
- Pre-check step documentado em ambos feature.md e resume.md
- Skip condition clara: empty OR <10 insertions/deletions
- Mirrors sync
- Build + tests 26/26

## Guards
- Pre-check é bash puro, zero tokens LLM
- Skip não pula EXECUTE — só pula o Haiku dispatch do gate
- Preservar Haiku dispatch intacto para casos que passam o pre-check
- NÃO introduzir dependência em /dev/null ou shell-specific

## Result

Pre-check block inserted in both `feature/SKILL.md` and `resume/SKILL.md`. Mirrors synced.

### Walkthrough

**Scenario 1 — Fresh spec, no commits since creation:**
`rtk git diff --stat HEAD -- <files>` returns empty output → pre-check rule: "Empty output → skip gate entirely" → proceed directly to EXECUTE. Zero Haiku dispatch, zero tokens spent.

**Scenario 2 — Small tweak committed (5 lines changed):**
`rtk git diff --stat HEAD -- <files>` returns `1 file changed, 3 insertions(+), 2 deletions(-)` → total 5 insertions/deletions < 10 → pre-check rule: "<10 total insertions/deletions → skip gate entirely" → proceed directly to EXECUTE. No Haiku dispatch.

**Scenario 3 — Substantial change committed (25 lines changed):**
`rtk git diff --stat HEAD -- <files>` returns `2 files changed, 18 insertions(+), 7 deletions(-)` → total 25 ≥ 10 → pre-check passes through → proceed with Haiku dispatch as normal. Gate runs at full cost, justified by meaningful changes.
