# Plano de Waves

### Stage: Plan
### Outcome: Active
### Flags: 
### Scope: full (wave plan)
### Total waves: 5

## Tabela de Waves

| Wave | Spec | Role | Depende de | Resumo |
|------|------|------|------------|--------|
| 1 | [[wave-1-general]] | general | — | DS foundation: rodar getdesign binance, consolidar tokens em style.css, deletar styles/theme.css, criar script check-pages-imports |
| 2 | [[wave-2-ui]] | ui | [[1]] | Primitives consolidados: criar PageSurface, mover ds/* para page/*, renomear MetricsPill->StatPill, deletar components/ds, find-replace imports |
| 3 | [[wave-3-ui]] | ui | [[2]] | Layout shell refit: AppShell, Sidebar, Topbar, SplitDetail alinhados ao DESIGN.md (ritmo vertical, type voltage) |
| 4 | [[wave-4-ui]] | ui | [[3]] | Pages high-traffic: Workspace, Specs, Economia, Knowledge consomem PageSurface + barril unificado |
| 5 | [[wave-5-ui]] | ui | [[4]] | Pages secondary: ProjectDetail, SpecDetail, Prd, Commands, Settings, Preferences, Home migrados ao mesmo padrao |
