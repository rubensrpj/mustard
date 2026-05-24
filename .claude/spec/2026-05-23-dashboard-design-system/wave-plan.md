# Plano de Waves

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full (wave plan)
### Total waves: 6
### Checkpoint: 2026-05-23T23:55:00Z

## Tabela de Waves

| Wave | Spec | Role | Modelo | Depende de | Resumo |
|------|------|------|--------|------------|--------|
| 1 | [[wave-1-general]] | general | opus | — | DS foundation: rodar getdesign binance, consolidar tokens em style.css, deletar styles/theme.css, criar script check-pages-imports |
| 2 | [[wave-2-ui]] | ui | opus | [[1]] | Primitives consolidados: criar PageSurface, mover ds/* para page/*, renomear MetricsPill->StatPill, deletar components/ds, find-replace imports |
| 3 | [[wave-3-ui]] | ui | opus | [[2]] | Layout shell refit: AppShell, Sidebar, Topbar, SplitDetail alinhados ao DESIGN.md (ritmo vertical, type voltage) |
| 4 | [[wave-4-ui]] | ui | opus | [[3]] | Folder-per-component + features/ namespace: mover components/{specs,workspace,economy,knowledge,prd,telemetry,amend,trace} para features/, cada .tsx vira pasta com index.tsx, relocar 10 strays do root, criar codemod + check-pages-no-inline-visual.mjs |
| 5 | [[wave-5-ui]] | ui | opus | [[4]] | Pages high-traffic: Workspace, Specs, Economia, Knowledge consomem PageSurface + caminhos novos (@/features/*) + limpeza de tokens fantasmas nas páginas |
| 6 | [[wave-6-ui]] | ui | opus | [[5]] | Pages secondary: ProjectDetail, SpecDetail, Prd, Commands, Settings, Preferences, Home migrados ao mesmo padrão |
