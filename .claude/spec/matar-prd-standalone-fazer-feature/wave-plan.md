---
id: wave.matar-prd-standalone-fazer-feature.plan
---

# Plano de Waves

## Tabela de Waves

| Wave | Spec | Papel | Depende de | Resumo |
|------|------|-------|------------|--------|
| 1 | [[wave-1-grill]] | grill | — | /feature grelha inline: glossary-coverage expõe termos fracos, mini-grill focado, escritor map-aware grava no CONTEXT.md do subprojeto |
| 2 | [[wave-2-purge-prd]] | purge-prd | — | Remover o PRD standalone: prd-build (rt), skill mustard:prd, e qualquer exposição em mcp; manter PRD_SECTIONS |
| 3 | [[wave-3-dashboard]] | dashboard | [[wave-1-grill]], [[wave-2-purge-prd]] | Rota PRD do dashboard vira porta GUI do fluxo /feature: dispara feature e produz spec rastreada, não rascunho de clipboard |

## Critérios de Aceitação
- **AC-5** — O escritor de termo grava no CONTEXT.md resolvido por CONTEXT-MAP. Command: `cargo test -p mustard-rt grill_capture`
- **AC-4** — feature/SKILL.md descreve o grill inline. Command: `rg -n "grelh|grill" apps/cli/templates/commands/mustard/feature/SKILL.md`
- **AC-2** — prd-build removido do rt. Command: `! rg -n "prd[-_]build|PrdBuild" apps/rt/src/commands`
- **AC-3** — Skill mustard:prd removido. Command: `test ! -e apps/cli/templates/commands/mustard/prd/SKILL.md`
- **AC-6** — O dashboard não invoca mais /mustard:prd. Command: `! rg -n "mustard:prd|/prd" apps/dashboard/src-tauri/src/prd_lapidator.rs`
