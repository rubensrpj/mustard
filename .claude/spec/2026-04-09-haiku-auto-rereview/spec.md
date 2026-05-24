# Enhancement: haiku-auto-rereview

## Summary
Adicionar heurística que downgrade automático de re-reviews para `model: "haiku"` quando a fix list do primeiro review é trivial (≤3 issues OU diff <20 linhas). Reviews iniciais permanecem no modelo default (sonnet/opus). Economia estimada: ~600 tokens por re-review, reduzindo custo total de fix loops.

## Why
Memory `reference_mustard_token_efficiency.md` mostra re-reviews custando 26-32K tokens mesmo com Haiku, e auditoria confirmou que `templates/commands/mustard/review/SKILL.md` tem model hardcoded (sem lógica condicional). Fix loops pós-review são o maior vilão de custo em features Full scope (170-266K tokens extras observados). Rápida vitória: re-reviews triviais não precisam de Opus/Sonnet.

## Boundaries
- `templates/commands/mustard/feature/SKILL.md` — review dispatch step
- `templates/commands/mustard/complete/SKILL.md` — review dispatch step
- `templates/commands/mustard/review/SKILL.md` — review SKILL itself
- `.claude/commands/mustard/feature/SKILL.md` — mirror
- `.claude/commands/mustard/complete/SKILL.md` — mirror
- `.claude/commands/mustard/review/SKILL.md` — mirror

## Checklist
### templates-impl Agent
- [x] Localizar onde review/re-review agents são despachados (grep "model:" nos 3 SKILL.md)
- [x] Desenhar heurística: `re_review = true AND (issue_count <= 3 OR lines_changed < 20) → model: "haiku"`, senão default
- [x] Adicionar instrução ao orchestrator em feature.md + complete.md: contar issues do review anterior (parser simples do return format "[Severity] File:Line") e decidir modelo antes do re-dispatch
- [x] Atualizar review/SKILL.md para documentar a heurística (seção "Model Selection")
- [x] Espelhar de `templates/` para `.claude/`
- [x] Build/type-check: `npm run build`
- [x] Validar que os 3 SKILL.md renderizam bem (sem markdown quebrado)

## Files (~6)
- `templates/commands/mustard/feature/SKILL.md` (modify)
- `templates/commands/mustard/complete/SKILL.md` (modify)
- `templates/commands/mustard/review/SKILL.md` (modify)
- `.claude/commands/mustard/feature/SKILL.md` (mirror)
- `.claude/commands/mustard/complete/SKILL.md` (mirror)
- `.claude/commands/mustard/review/SKILL.md` (mirror)

## Acceptance
- Heurística documentada em review/SKILL.md
- feature.md + complete.md referenciam a heurística antes de despachar re-review
- Mudanças espelhadas em `.claude/`
- Build limpo
- Heurística é textual (instrução para orchestrator), não código — sem hook novo

## Result
- `templates/commands/mustard/review/SKILL.md` lines 83-95 — Model Selection section added
- `templates/commands/mustard/feature/SKILL.md` line 191 — re-review heuristic added to step 9
- `templates/commands/mustard/complete/SKILL.md` lines 33-35 — re-review model selection added
- `.claude/commands/mustard/review/SKILL.md` — mirrored
- `.claude/commands/mustard/feature/SKILL.md` — mirrored
- `.claude/commands/mustard/complete/SKILL.md` — mirrored
- Build: PASS (tsc clean)
