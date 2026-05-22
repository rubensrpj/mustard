# Review Plan — Wave network como padrão Mustard

### Parent: [[2026-05-20-mustard-wave-network-standard]]
### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: review
### Checkpoint: 2026-05-20T22:55:00Z
### Lang: pt

## PRD

## Contexto

Plano de Review **declarado upfront** (SDD), executado pela SKILL `/mustard:review` após todas as waves de execução marcarem `completed` mas antes do QA. O verdict final é gravado em `review/verdict.md` no mesmo dir.

## Checklist por categoria

Cada categoria é um item binário (passa / não passa). Reviewer (modelo `sonnet`, definido pelo orquestrador) lê o diff agregado da spec inteira e emite verdict por categoria.

- [ ] Categoria 1 — Correctness: implementação corresponde ao declarado em cada wave spec.md? Sem desvios silenciosos?
- [ ] Categoria 2 — Boundaries: edits respeitam os "Limites" de cada wave? Sem vazamento out-of-scope?
- [ ] Categoria 3 — Wikilinks integridade: `wikilink-extract` rodando contra esta spec não retorna `orphans` para nomes que existem (parser correto)?
- [ ] Categoria 4 — Cross-wave memory: cada wave>1 efetivamente recebeu `{cross_wave_memory}` no prompt (verifica em `events.agent.prompt.injected` se logger registrou)?
- [ ] Categoria 5 — Modelo do orquestrador: dispatches efetivamente usaram o modelo declarado no `wave-plan.md` (verifica events `pipeline.task.dispatch.model`)?
- [ ] Categoria 6 — Métricas funcionais: páginas Economia/Quality do dashboard renderizam números não-zero quando há eventos reais, agrupados por parent?
- [ ] Categoria 7 — Karpathy guidelines: edits respeitam surgical/sem refactor extra, sem comments soltos, sem error handling defensivo?

## Acceptance Criteria

- [ ] AC-1: `verdict.md` existe após review — Command: `bash -c 'test -f "$(find .claude/spec -path "*2026-05-20-mustard-wave-network-standard/review/verdict.md" | head -1)"'`
- [ ] AC-2: Cada categoria tem entrada no verdict — Command: `bash -c 'f=$(find .claude/spec -path "*2026-05-20-mustard-wave-network-standard/review/verdict.md" | head -1); for i in 1 2 3 4 5 6 7; do grep -q "Categoria $i" "$f" || exit 1; done'`

## Saída esperada

Arquivo `review/verdict.md` (criado pela SKILL `/review`) com formato:

```
# Verdict — Wave network como padrão Mustard
Status: APPROVED | REJECTED | APPROVED_WITH_CONCERNS
Reviewer: sonnet
Date: <ISO>

## Categoria 1 — Correctness: PASS | FAIL
{notes}

(... 7 categorias ...)

## Critical issues (se REJECTED)
- ...
```

## Network

- Parent: [[2026-05-20-mustard-wave-network-standard]]
- Roda depois de: [[wave-1-rt-infra]], [[wave-2-skill-template]], [[wave-3-dashboard-graph]], [[wave-4-metrics-diagnose-fix]]
- Desbloqueia: [[qa]] (só roda QA se review APPROVED ou APPROVED_WITH_CONCERNS)
