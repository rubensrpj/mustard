---
id: wave.dashboard-aba-atividade-agrupar-trabalho.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave-1-backend]] | backend | — | Backend Tauri le pipeline.kind e expoe kind + narrativa do pedido por unidade de trabalho; deriva o agrupamento |
| 2 | [[wave-2-frontend]] | frontend | [[wave-1-backend]] | Aba Atividade (substitui Specs): agrupa por rotulo humano mapeado do kind + cada item mostra pedido original + narrativa |

## Acceptance Criteria
- **AC-3** — backend le o evento de tipo. Command: `git grep --no-index -q "pipeline.kind" -- apps/dashboard/src-tauri`
- **AC-1** — build verde. Command: `cargo build`
- **AC-2** — testes verdes. Command: `cargo test`
- **AC-4** — aba Atividade existe. Command: `git grep --no-index -q "Atividade" -- apps/dashboard/src/pages`
- **AC-5** — rotulos humanos do kind. Command: `git grep --no-index -q "Nova funcionalidade" -- apps/dashboard/src`

<!-- wikilinks-footer-start -->
- [wave-1-backend](?) ⚠ unresolved
- [wave-2-frontend](?) ⚠ unresolved
<!-- wikilinks-footer-end -->