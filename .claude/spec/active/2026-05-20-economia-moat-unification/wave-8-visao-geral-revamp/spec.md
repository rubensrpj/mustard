# Wave 8 — Visão Geral revamp (cosméticos + multi-spec + i18n + bug fixes)

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: queued
### Phase: PLAN
### Scope: full (wave)
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt

## PRD

A crítica original do usuário sobre a Visão Geral redesenhada (entregue na spec `2026-05-20-dashboard-visual-overview`) identificou 7 problemas. Desses, **2 foram cobertos** pelas waves W1-W7 desta feature: ECONOMIA DE TOKENS vazia (W1-W4-W7 atacam a raiz) e Events feed linear (W6 substitui por trace hierárquico). Os **5 restantes** são UI cosmética da Visão Geral que esta wave entrega: hero útil só pra 1 spec (precisa ser lista compacta multi-spec, sem "eventos/min" técnico, com "ECONOMIZADOS HOJE" movido pro card Economia); SPECS POR STATUS dividindo width com Economia e labels hard-coded em PT (precisa full-width + i18n via Preferences); MonthCalendar grande e inútil (precisa virar `<StatusCounters>` compacto com concluídas/pendentes/QA/blocked); Alerts + Files columns finos verticais sem ícones (precisa split 50/50 com ícones Lucide coloridos); bug do `top_files_today` esvaziando pós-CLOSE. Consome primitivas DS da W5. Não bloqueia W6/W7 — paralelizável.

## Usuários/Stakeholders

Operador (você) abrindo a Visão Geral em sessões reais com 2+ specs ativas, querendo entender contagens e alertas em ≤3s sem inferência mental.

## Métrica de sucesso

Operador abre `/` (com 2+ specs ativas) e responde sem hover/scroll: (a) quantas pipelines rodando agora e em que fase cada uma; (b) contagem por status (specs concluídas/QA/blocked/etc.) no lugar do calendário; (c) último alerta com ícone visual indicando severidade; (d) arquivos tocados continua respondendo mesmo após spec fechar; (e) labels obedecem `Preferences.lang`.

## Não-Objetivos

- Não rescrever os 5 `Workspace*` da Wave 3 da spec anterior — apenas ajustar `Workspace.tsx` e adicionar componentes novos.
- Não tocar Sidebar, Topbar, outras pages (Specs, Knowledge, Settings, Preferences).
- Não migrar páginas legadas para i18n — apenas Visão Geral consome o provider novo. Migração lazy nas próximas features.
- Não criar persistência de filtros (sessão única).
- Não criar `<MonthCalendar>` v2 — calendar fica disponível como drill-in opcional, fora do primeiro fold (decisão de UX: dado de baixa densidade não merece prime real estate).

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: Build passa — Command: `pnpm --filter mustard-dashboard build`
- [ ] AC-2: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [ ] AC-3: i18n provider existe e bindado em Preferences — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/lib/i18n.ts'))throw new Error('i18n provider missing');const t=require('fs').readFileSync('apps/dashboard/src/lib/i18n.ts','utf8');if(!/preferences|usePreference/i.test(t))throw new Error('i18n not bound to Preferences')"`
- [ ] AC-4: Hero multi-spec — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/workspace/WorkspaceHero.tsx','utf8');if(!/map|forEach/.test(t))throw new Error('Hero must iterate pipelines (multi-spec)')"`
- [ ] AC-5: StatusCounters substituiu calendar — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');if(t.includes('WorkspaceMonthCalendar'))throw new Error('MonthCalendar still imported');if(!t.includes('WorkspaceStatusCounters'))throw new Error('StatusCounters not imported')"`
- [ ] AC-6: Alerts+Files split 50/50 — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');if(!/w-1\\/2|grid-cols-2/.test(t))throw new Error('Alerts+Files not split 50/50')"`
- [ ] AC-7: top_files_today não esvazia pós-CLOSE — Command: `cargo test -p mustard-dashboard test_top_files_today_post_close --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [ ] AC-8: "ECONOMIZADOS HOJE" não está mais no StatusBar — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/workspace/WorkspaceStatusBar.tsx','utf8');if(/economizados\\s+hoje|economizados_hoje|savedToday/i.test(t))throw new Error('savedToday still in StatusBar')"`
- [ ] AC-9: SpecsByStatus ocupa full-width (sem className col-span-2) — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');const m=t.match(/<WorkspaceSpecsByStatus[^>]*>/);if(m && /col-span-2/.test(m[0]))throw new Error('SpecsByStatus still wrapped in col-span-2')"`

## Plano

## Informações da Entidade

Sem nova entidade de domínio. Apenas componentes UI + 1 hook backend pra fix do bug.

## Arquivos (~8)

```
apps/dashboard/src/lib/i18n.ts                              (new — provider + dictionary PT/EN)
apps/dashboard/src/components/workspace/WorkspaceHero.tsx   (new — substitui StatusBar+PipelineTimeline única, lista multi-spec)
apps/dashboard/src/components/workspace/WorkspaceStatusCounters.tsx (new — substitui MonthCalendar)
apps/dashboard/src/components/workspace/WorkspaceStatusBar.tsx (modify — remover bloco "ECONOMIZADOS HOJE")
apps/dashboard/src/components/workspace/WorkspaceFilesRanking.tsx (modify — usar ícones Lucide; consume novo backend que não esvazia)
apps/dashboard/src/components/workspace/WorkspaceAlertsColumn.tsx (modify — usar ícones Lucide por severidade via DS primitives)
apps/dashboard/src/pages/Workspace.tsx                      (modify — layout: novo Hero + remove calendar + split Alerts+Files)
apps/dashboard/src-tauri/src/spec_views.rs                  (modify — fix dashboard_workspace_summary para top_files_today não filtrar por session_id corrente)
```

## Tarefas

### Frontend i18n Agent

- [ ] Criar `apps/dashboard/src/lib/i18n.ts`: provider mínimo (sem `i18next` — usar Map<string,Record<lang,string>> caseiro) bindado em `usePreferences` zustand slice existente. Suporta `pt` e `en`. Expor `useTranslate()` hook que retorna `t(key, fallback?)`.
- [ ] Migrar labels da Visão Geral para o provider: `Hoje`, `7d`, `30d`, `Ver detalhes`, `Carregando`, `Sem economia registrada`, `Atividade do mês`, `Feed de eventos`, `Alertas`, `Arquivos mais tocados hoje`.

### Frontend UI Agent

- [ ] Criar `WorkspaceHero.tsx`: consome `useWorkspaceSummary` (existente) que já lista N pipelines ativos; renderiza linha compacta por pipeline (nome + chip da fase atual + duração + tokens consumidos), ordenado por última atividade desc. Substitui `WorkspaceStatusBar` + `PipelineTimeline` single-pipeline.
- [ ] Criar `WorkspaceStatusCounters.tsx`: consome `useWorkspaceSummary.tracks`; renderiza 5 contadores grandes lado a lado (`Concluídas`, `Em QA`, `Em Review`, `Pendentes`, `Bloqueadas`) com badges semânticos. Ocupa fração do espaço do calendário antigo.
- [ ] Editar `WorkspaceStatusBar.tsx`: remover bloco "ECONOMIZADOS HOJE" (move pra card Economia que já existe na própria página; também aparece em `/economia` via W7).
- [ ] Editar `WorkspaceFilesRanking.tsx`: trocar texto de rank por `<MetricsPill>` da DS; ícone `FileCode` ou similar do Lucide ao lado de cada path.
- [ ] Editar `WorkspaceAlertsColumn.tsx`: trocar texto "QA falhou" / "blocked" / "Review rejeitado" por `<Badge>` semântica + ícone Lucide (`AlertTriangle`/`Ban`/`XCircle`) colorido pela severidade. Reusa primitivas DS da W5.
- [ ] Editar `Workspace.tsx`: remover `<WorkspaceStatusBar>` + `<PipelineTimeline>` substituindo por `<WorkspaceHero>`; remover `<WorkspaceMonthCalendar>` substituindo por `<WorkspaceStatusCounters>`; mover `<WorkspaceSpecsByStatus>` pra ocupar full-width (sem `col-span-2`); juntar `<WorkspaceTokenSummary>` em coluna direita ou abaixo (decisão fina durante implementação). Bottom: `<div className="grid grid-cols-2 gap-6">` com `WorkspaceAlertsColumn` à esquerda e `WorkspaceFilesRanking` à direita (split 50/50).

### Backend Bug Fix Agent

- [ ] Editar `apps/dashboard/src-tauri/src/spec_views.rs::dashboard_workspace_summary` — investigar a query do `top_files_today`. Hoje provavelmente filtra `WHERE date(ts) = today AND session_id = ?` — remover filtro por `session_id` (deve contar TODAS as edições do dia, não só da sessão atual). Após CLOSE de spec, sessão muda mas dia continua — query deve permanecer válida.
- [ ] Adicionar teste de regressão `test_top_files_today_post_close` em `apps/dashboard/src-tauri/src/tests/` (ou módulo de teste inline): popula 3 events `tool.use` com paths em 2 session_ids diferentes do mesmo dia, valida que query devolve agregado dos 2 (não só do session atual).

### Validação final

- [ ] Rodar `pnpm --filter mustard-dashboard build` + `pnpm --filter mustard-dashboard exec tsc --noEmit` + `cargo check -p mustard-dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml`. Todos passar.

## Dependências

- [[wave-5-ds-foundation]]: primitivas DS para badges semânticos + MetricsPill + ícones consistentes.

(Não depende de W6 nem W7 — paralelizável com ambas.)

## Network

- Parent: [[2026-05-20-economia-moat-unification]]
- Depende de: [[wave-5-ds-foundation]]
- Paralela a: [[wave-6-trace-viewer]], [[wave-7-economia-page]]
- Desbloqueia: QA (Wave 10) → CLOSE
- Grava memória: `{components_added: ['WorkspaceHero','WorkspaceStatusCounters'], components_removed_from_layout: ['WorkspaceStatusBar','PipelineTimeline-single','WorkspaceMonthCalendar'], i18n_keys: [...], bug_fixes: ['top_files_today_post_close']}`

## Limites

Em escopo: `apps/dashboard/src/lib/i18n.ts` (novo), `apps/dashboard/src/components/workspace/Workspace{Hero,StatusCounters}.tsx` (novos), `apps/dashboard/src/components/workspace/Workspace{StatusBar,FilesRanking,AlertsColumn}.tsx` (edit), `apps/dashboard/src/pages/Workspace.tsx` (edit), `apps/dashboard/src-tauri/src/spec_views.rs` (edit — só `dashboard_workspace_summary`).

Fora de escopo: outras páginas (`Specs`, `Economia`, `Knowledge`, `Settings`, `Preferences`); outros Tauri commands; Sidebar/Topbar; migração de outras páginas para i18n (lazy).
