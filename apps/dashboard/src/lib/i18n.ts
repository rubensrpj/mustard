// SPEC LANG: pt-allowed — in-house i18n catalogue. pt-BR strings are data, not narrative.
// Lightweight in-house i18n provider used across the dashboard.
//
// Background — two i18n surfaces coexist in this repo:
//
//   1. `src/i18n.ts` — i18next instance, namespace `common`. Older pages
//      (Sidebar projects/menu, Settings, Preferences, projects toasts) consume
//      it via `useTranslation()` from `react-i18next`.
//   2. THIS module — a flat `Map<string, Record<'pt-BR'|'en-US', string>>` bound to
//      the same Preferences slice (`useStore((s) => s.language)`). It started
//      life serving the Visão Geral revamp (Wave 8) and the W2 i18n-audit
//      (spec `2026-05-21-dashboard-i18n-and-phase-unify`) generalized it: the
//      Sidebar nav, route titles, Specs/Knowledge headers and shared
//      action/phase/count vocab all live here now, with `t(key)` exported as
//      the canonical surface from `@/lib/i18n`.
//
// We deliberately did NOT collapse the two — keeping `src/i18n.ts` intact
// preserves the Settings/Preferences/projects strings and their interpolation
// helpers (`{{name}}`, `_one` plurals) without rewriting consumers. The two
// dictionaries stay in sync via the shared `useStore.language` slice.
//
// Reactivity model
// ----------------
// `useT()` is the React hook. It calls `useStore((s) => s.language)` so any
// component that uses it re-renders on language change automatically. The
// imperative `t()` reads `useStore.getState().language` synchronously — fine
// for one-off callers outside the React tree (toast text, label maps) but
// NOT reactive on its own; pair it with the hook in render paths.
//
// Resolution order: active language → PT fallback → EN fallback → key.
// Surfacing the raw key in dev keeps missing translations visible instead of
// collapsing to an empty string.

import { useStore } from "@/lib/store";

// BCP-47 only (see memory `project_locale_codes`). Legacy short codes
// `pt`/`en` are NOT a valid public type — but the in-memory dictionary
// rows below use the short keys for compactness. Resolution code maps
// BCP-47 → short keys at the boundary.
export type Lang = "pt-BR" | "en-US";

/** Internal short keys used inside the DICTIONARY rows for compactness. */
type DictKey = "pt" | "en";

/** A single translation row keyed by the internal short codes. */
export type TranslationRow = Record<DictKey, string>;

/** Map a BCP-47 [`Lang`] onto the internal dictionary short key. */
function dictKey(lang: Lang): DictKey {
  return lang === "pt-BR" ? "pt" : "en";
}

/**
 * Flat dictionary. Entries are grouped by surface (`sidebar.*`, `route.*`,
 * `action.*`, etc.) so future migrations stay grep-able. New keys live here —
 * consumers reach them via `t(key)` or `useT()`.
 */
const DICTIONARY = new Map<string, TranslationRow>([
  // Period segmented control (WorkspaceSpecsByStatus)
  ["period.today", { pt: "Hoje", en: "Today" }],
  ["period.7d", { pt: "7d", en: "7d" }],
  ["period.30d", { pt: "30d", en: "30d" }],

  // Generic verbs / states used across the page.
  ["common.loading", { pt: "Carregando…", en: "Loading…" }],
  ["common.viewDetails", { pt: "Ver detalhes →", en: "View details →" }],
  ["common.empty", { pt: "Sem dados disponíveis", en: "No data available" }],

  // Workspace / Visão Geral section titles.
  ["workspace.title", { pt: "Visão Geral", en: "Overview" }],
  ["workspace.subtitle", { pt: "Sala de operações multi-track", en: "Multi-track operations room" }],
  ["workspace.activePipelines", { pt: "Pipelines ativos", en: "Active pipelines" }],
  ["workspace.statusCounters", { pt: "Specs por estado", en: "Specs by state" }],
  ["workspace.specsByStatus", { pt: "Specs por status", en: "Specs by status" }],
  ["workspace.alerts", { pt: "Alertas", en: "Alerts" }],
  ["workspace.filesRanking", { pt: "Arquivos mais tocados hoje", en: "Most-touched files today" }],
  ["workspace.tokenSummary", { pt: "Economia de tokens", en: "Token savings" }],
  ["workspace.monthActivity", { pt: "Atividade do mês", en: "Month activity" }],
  ["workspace.eventsFeed", { pt: "Feed de eventos", en: "Events feed" }],

  // Status bucket labels (StatusCounters).
  ["status.completed", { pt: "Concluídas", en: "Completed" }],
  ["status.qa", { pt: "Em QA", en: "In QA" }],
  ["status.reviewing", { pt: "Em Review", en: "In Review" }],
  ["status.pending", { pt: "Pendentes", en: "Pending" }],
  ["status.blocked", { pt: "Bloqueadas", en: "Blocked" }],
  ["status.implementing", { pt: "Implementando", en: "Implementing" }],
  ["status.no_data", { pt: "Sem dados", en: "No data" }],

  // Hero empty / labels.
  ["hero.empty", { pt: "Nenhum pipeline ativo", en: "No active pipeline" }],
  ["hero.emptyHint", { pt: "Inicie uma pipeline para vê-la aqui.", en: "Start a pipeline to see it here." }],
  ["hero.duration", { pt: "duração", en: "duration" }],
  ["hero.tokens", { pt: "tokens", en: "tokens" }],

  // Sidebar nav (W2 audit). Aliases that mirror the i18next `nav.*` entries so
  // both surfaces stay coherent under a single language switch.
  ["sidebar.overview", { pt: "Visão Geral", en: "Overview" }],
  ["sidebar.specs", { pt: "Specs", en: "Specs" }],
  ["sidebar.economy", { pt: "Economia", en: "Economy" }],
  ["sidebar.knowledge", { pt: "Conhecimento", en: "Knowledge" }],
  ["sidebar.commands", { pt: "Comandos", en: "Commands" }],
  ["sidebar.preferences", { pt: "Preferências", en: "Preferences" }],
  ["sidebar.settings", { pt: "Configurações", en: "Settings" }],
  ["sidebar.sessions", { pt: "Sessões", en: "Sessions" }],
  ["sidebar.activity", { pt: "Atividade", en: "Activity" }],
  ["sidebar.telemetry", { pt: "Telemetria", en: "Telemetry" }],
  ["sidebar.quality", { pt: "Qualidade", en: "Quality" }],
  ["sidebar.add_project", { pt: "Adicionar projeto", en: "Add project" }],
  ["sidebar.tools", { pt: "Ferramentas", en: "Tools" }],

  // Route headers (PageHeader title/subtitle pairs).
  ["route.specs.title", { pt: "Specs", en: "Specs" }],
  ["route.specs.subtitle", { pt: "Lista e drill-down por spec", en: "List and per-spec drill-down" }],

  // Specs dense-list (spec-lifecycle-unification W3) — Stage group headers.
  ["route.specs.groups.analyze", { pt: "Analisando", en: "Analyzing" }],
  ["route.specs.groups.plan", { pt: "Planejando", en: "Planning" }],
  ["route.specs.groups.execute", { pt: "Executando", en: "Executing" }],
  ["route.specs.groups.qa_review", { pt: "Validando", en: "Reviewing" }],
  ["route.specs.groups.awaiting_close", { pt: "Aguardando fechamento", en: "Awaiting close" }],
  ["route.specs.groups.close", { pt: "Fechadas", en: "Closed" }],
  ["route.specs.groups.cancelled", { pt: "Canceladas", en: "Cancelled" }],
  ["route.specs.groups.abandoned", { pt: "Abandonadas", en: "Abandoned" }],
  ["route.specs.groups.superseded", { pt: "Substituídas", en: "Superseded" }],
  ["route.specs.groups.absorbed", { pt: "Absorvidas", en: "Absorbed" }],
  // Expandable-tree child kind tags + empty/error/filters.
  ["route.specs.child.wave", { pt: "onda", en: "wave" }],
  ["route.specs.child.ac", { pt: "AC", en: "AC" }],
  ["route.specs.child.sub_spec", { pt: "sub-spec", en: "sub-spec" }],
  ["route.specs.empty_group", { pt: "0", en: "0" }],
  ["route.specs.children_empty", { pt: "Sem ondas, ACs ou sub-specs.", en: "No waves, ACs or sub-specs." }],
  ["route.specs.children_error", { pt: "Não foi possível carregar os filhos.", en: "Could not load children." }],
  ["route.specs.filter.ativas", { pt: "Ativas", en: "Active" }],
  ["route.specs.filter.suspeitas", { pt: "Suspeitas", en: "Flagged" }],
  ["route.specs.filter.encerradas", { pt: "Encerradas", en: "Closed" }],
  ["route.knowledge.title", { pt: "Conhecimento", en: "Knowledge" }],
  ["route.knowledge.subtitle", { pt: "O que o Mustard aprendeu neste workspace", en: "What Mustard learned in this workspace" }],

  // Breadcrumb segments.
  ["breadcrumb.workspace", { pt: "Workspace", en: "Workspace" }],
  ["breadcrumb.mustard", { pt: "Mustard", en: "Mustard" }],

  // Common actions.
  ["action.add", { pt: "Adicionar", en: "Add" }],
  ["action.refresh", { pt: "Atualizar", en: "Refresh" }],
  ["action.close", { pt: "Fechar", en: "Close" }],
  ["action.reload_projects", { pt: "Recarregar projetos", en: "Reload projects" }],

  // Empty states / counts shared across pages.
  ["empty.no_events", { pt: "Pipeline ainda sem eventos", en: "Pipeline has no events yet" }],
  ["count.acs", { pt: "ACs", en: "ACs" }],
  ["count.files", { pt: "arquivos", en: "files" }],
  ["count.tools", { pt: "tools", en: "tools" }],

  // Pipeline phases (canonical labels — single source of truth for any chip,
  // breadcrumb or filter that renders a phase name).
  ["phase.analyze", { pt: "Analisar", en: "Analyze" }],
  ["phase.plan", { pt: "Planejar", en: "Plan" }],
  ["phase.execute", { pt: "Executar", en: "Execute" }],
  ["phase.review", { pt: "Revisar", en: "Review" }],
  ["phase.qa", { pt: "QA", en: "QA" }],
  ["phase.close", { pt: "Fechar", en: "Close" }],

  // Drawer / panel affordances.
  ["drawer.pin", { pt: "Fixar painel", en: "Pin panel" }],
  ["drawer.unpin", { pt: "Soltar painel", en: "Unpin panel" }],

  // Wave-6 — workspace health card (hygiene observability).
  ["workspace.health.title", { pt: "Saúde do workspace", en: "Workspace health" }],
  ["workspace.health.active", { pt: "Ativas", en: "Active" }],
  ["workspace.health.suspects", { pt: "Suspeitas", en: "Suspects" }],
  ["workspace.health.autoclose_today", { pt: "Auto-fechadas hoje", en: "Auto-closed today" }],
  ["workspace.health.blocked", { pt: "Bloqueadas", en: "Blocked" }],
  ["workspace.health.wave_failed", { pt: "Wave failed", en: "Wave failed" }],
  ["workspace.health.followup_open", { pt: "Follow-up", en: "Follow-up" }],
  ["workspace.health.last_run", { pt: "Última verificação há {time}", en: "Last check {time} ago" }],

  // Wave-6 — spec badges (hygiene flags).
  ["specs.badge.blocked", { pt: "bloqueada", en: "blocked" }],
  ["specs.badge.wave_failed", { pt: "wave failed", en: "wave failed" }],
  ["specs.badge.followup", { pt: "follow-up", en: "follow-up" }],
  ["specs.badge.suspect", { pt: "suspeita", en: "suspect" }],
  ["specs.badge.auto_closed", { pt: "auto-fechada", en: "auto-closed" }],

  // Wave-6 — "Suspeitas" filter pill (populated by hygiene suspects).
  ["specs.filter.suspects", { pt: "Suspeitas", en: "Suspects" }],

  // ── Spec kebab action menu (SpecActionMenu) ──────────────────────────────
  // "Reabrir" shows only for terminal/closed specs (inverse of "Fechar");
  // "Fechar" + "Remover" for active ones.
  ["specs.action.reopen", { pt: "Reabrir", en: "Reopen" }],
  ["specs.action.close", { pt: "Fechar", en: "Close" }],
  ["specs.action.remove", { pt: "Remover", en: "Remove" }],

  // ── Shared empty-state copy (template-agnostic-audit) ───────────────────
  // Reused by every page that gates on `projectsRoot` / `activeWorkspaceId`.
  ["empty.noRoot.title", { pt: "Diretório de projetos não configurado", en: "Projects directory not configured" }],
  ["empty.noRoot.description", { pt: "Vá em Configurações e aponte para a pasta onde estão seus repos.", en: "Go to Settings and point Mustard at the folder where your repos live." }],
  ["empty.noRoot.descriptionSettings", { pt: "Vá em Settings e aponte para a pasta onde estão seus repos.", en: "Go to Settings and point Mustard at the folder where your repos live." }],
  ["empty.noWorkspace.title", { pt: "Selecione um workspace", en: "Select a workspace" }],
  ["empty.noWorkspace.description", { pt: "Use o seletor na sidebar para escolher um projeto.", en: "Use the sidebar picker to choose a project." }],
  ["empty.noWorkspace.descriptionTop", { pt: "Use o seletor no topo da sidebar para escolher um projeto e ver o que ele aprendeu.", en: "Use the picker at the top of the sidebar to choose a project and see what it learned." }],
  ["common.loadingDots", { pt: "Carregando…", en: "Loading…" }],

  // ── Home page ─────────────────────────────────────────────────────────
  ["home.configureRoot.title", { pt: "Configure o diretório de projetos", en: "Configure the projects directory" }],
  ["home.configureRoot.body.before", { pt: "Vá em ", en: "Go to " }],
  ["home.configureRoot.body.linkLabel", { pt: "Settings", en: "Settings" }],
  ["home.configureRoot.body.after", { pt: " e aponte para a pasta onde estão seus repos.", en: " and point Mustard at the folder where your repos live." }],
  ["home.discovering", { pt: "Descobrindo projetos…", en: "Discovering projects…" }],
  ["home.noProjects.title", { pt: "Nenhum projeto encontrado", en: "No projects found" }],
  ["home.noProjects.body.before", { pt: "Não encontramos projetos em ", en: "No projects were found in " }],
  ["home.noProjects.body.after", { pt: ".", en: "." }],
  ["home.workspace.noEvents.before", { pt: "Este workspace ainda não emitiu eventos. Rode um pipeline (", en: "This workspace has not emitted any events yet. Run a pipeline (" }],
  ["home.workspace.noEvents.middle", { pt: ", ", en: ", " }],
  ["home.workspace.noEvents.after", { pt: ") para popular os dados.", en: ") to populate the data." }],
  ["home.activePipelines", { pt: "Pipelines ativos", en: "Active pipelines" }],
  ["home.noActivePipeline", { pt: "Nenhum pipeline ativo.", en: "No active pipeline." }],
  ["home.todayDigest", { pt: "Resumo de hoje", en: "Today's digest" }],
  ["home.portfolio.title", { pt: "Portfólio", en: "Portfolio" }],
  ["home.portfolio.subtitle", { pt: "Visão consolidada de todos os projetos descobertos no diretório raiz.", en: "Consolidated view of every project discovered under the root directory." }],
  ["home.portfolio.noPipelines", { pt: "Nenhuma pipeline em execução.", en: "No pipelines currently running." }],
  ["home.portfolio.projects", { pt: "Projetos", en: "Projects" }],

  // ── Workspace page ────────────────────────────────────────────────────
  ["workspace.editorialSubtitle", { pt: "Visão geral das pipelines ativas, saúde do projeto e atividade recente para {name}.", en: "Overview of active pipelines, project health and recent activity for {name}." }],

  // ── Specs page ────────────────────────────────────────────────────────
  ["specs.editorialTitle", { pt: "Specs", en: "Specs" }],
  ["specs.editorialSubtitle", { pt: "Lista de specs do workspace agrupadas por estágio. Use os filtros abaixo para isolar por estado, janela de tempo ou nome.", en: "List of workspace specs grouped by stage. Use the filters below to isolate by state, time window or name." }],
  ["specs.section.specs", { pt: "Specs", en: "Specs" }],
  ["specs.empty.noneFound.title", { pt: "Nenhuma spec encontrada", en: "No specs found" }],
  ["specs.empty.noneFound.description", { pt: "Ajuste os filtros ou rode uma pipeline com /mustard:feature.", en: "Adjust the filters or run a pipeline with /mustard:feature." }],
  ["specs.quickOpen.title", { pt: "Abrir spec em nova aba", en: "Open spec in a new tab" }],
  ["specs.quickOpen.placeholder", { pt: "Buscar por nome…", en: "Search by name…" }],
  ["specs.quickOpen.searchAria", { pt: "Buscar specs", en: "Search specs" }],
  ["specs.quickOpen.empty", { pt: "Nenhuma spec encontrada.", en: "No specs found." }],
  ["specs.filterBar.searchAria", { pt: "Buscar specs por nome", en: "Search specs by name" }],
  ["specs.filterBar.date.today", { pt: "Hoje", en: "Today" }],
  ["specs.filterBar.date.all", { pt: "Todas", en: "All" }],
  // Column headers for the spec rows (a discreet header line per group).
  ["specs.col.model", { pt: "Modelo", en: "Model" }],
  ["specs.col.waves", { pt: "Ondas", en: "Waves" }],
  ["specs.col.ac", { pt: "Critérios", en: "Criteria" }],
  ["specs.col.duration", { pt: "Duração", en: "Duration" }],
  ["specs.col.created", { pt: "Criada em", en: "Created" }],
  ["specs.col.stalledFor", { pt: "Parada há", en: "Idle for" }],
  // Plan-staleness "Reanalisar" affordance (planning rows).
  ["specs.staleness.button", { pt: "Reanalisar", en: "Re-analyze" }],
  ["specs.staleness.checking", { pt: "Analisando…", en: "Checking…" }],
  ["specs.staleness.stale", { pt: "obsoleto", en: "stale" }],
  ["specs.staleness.fresh", { pt: "ok", en: "ok" }],
  ["specs.staleness.unknown", { pt: "?", en: "?" }],
  // Tooltip evidence for the staleness verdict (lists the files that drove it).
  ["specs.staleness.missing", { pt: "Sumiram", en: "Missing" }],
  ["specs.staleness.changed", { pt: "Mudaram", en: "Changed" }],
  ["specs.staleness.age", { pt: "Idade do plano", en: "Plan age" }],
  ["specs.staleness.days", { pt: "dias", en: "days" }],
  ["specs.staleness.planDate", { pt: "Data do plano", en: "Plan date" }],

  // ── Activity page ─────────────────────────────────────────────────────
  // Replaces the Specs nav entry: groups every session by the human-readable
  // work TYPE (mapped from `category`, the `pipeline.kind` work-type signal),
  // titled by the original request, with the run narrative on drill-in.
  ["activity.editorialTitle", { pt: "Atividade", en: "Activity" }],
  ["activity.editorialSubtitle", { pt: "Trabalho deste projeto agrupado por tipo. Abra um item para ver a narrativa: o pedido, as fases, as mudanças e o desfecho.", en: "Work in this project grouped by type. Open an item to see its narrative: the request, the phases, the changes and the outcome." }],
  ["activity.editorialSubtitle.named", { pt: "Trabalho de {name} agrupado por tipo — abra um item para ver a narrativa.", en: "Work in {name} grouped by type — open an item to see its narrative." }],
  ["activity.empty.noProject.title", { pt: "Nenhum projeto ativo", en: "No active project" }],
  ["activity.empty.noProject.description", { pt: "Selecione um projeto na barra lateral para ver sua atividade.", en: "Select a project in the sidebar to see its activity." }],
  ["activity.empty.none.title", { pt: "Sem atividade registrada", en: "No activity recorded" }],
  ["activity.empty.none.description", { pt: "Nenhum trabalho foi registrado neste projeto ainda. Descreva o que você quer fazer para começar.", en: "No work has been recorded in this project yet. Describe what you want to do to get started." }],
  ["activity.untitled", { pt: "(pedido não capturado)", en: "(request not captured)" }],
  ["activity.unattributed", { pt: "(sessão não atribuída)", en: "(unattributed session)" }],
  ["activity.narrative.request", { pt: "Pedido", en: "Request" }],
  ["activity.narrative.changes", { pt: "Mudanças", en: "Changes" }],
  ["activity.narrative.outcome", { pt: "Desfecho", en: "Outcome" }],
  ["activity.narrative.openTrace", { pt: "Ver narrativa completa (fases, ferramentas e diffs)", en: "Open full narrative (phases, tools and diffs)" }],
  ["activity.outcome.open", { pt: "Em andamento", en: "In progress" }],
  ["activity.outcome.closed", { pt: "Encerrado", en: "Closed" }],
  // Human work-TYPE labels mapped from `category` (the kind). Unmapped
  // categories fall back to their capitalised own value.
  ["activity.kind.feature", { pt: "Nova funcionalidade", en: "New feature" }],
  ["activity.kind.task", { pt: "Ajuste", en: "Adjustment" }],
  ["activity.kind.bugfix", { pt: "Correção", en: "Bugfix" }],
  ["activity.kind.tactical-fix", { pt: "Mudança rápida", en: "Quick fix" }],
  ["activity.kind.followup", { pt: "Follow-up", en: "Follow-up" }],
  ["activity.kind.analyze", { pt: "Investigação", en: "Investigation" }],
  ["activity.kind.knowledge", { pt: "Conhecimento", en: "Knowledge" }],
  ["activity.kind.scan", { pt: "Varredura", en: "Scan" }],
  ["activity.kind.qa", { pt: "QA", en: "QA" }],
  ["activity.kind.outros", { pt: "Outros", en: "Other" }],
  ["activity.kind.__null__", { pt: "Avulsas (sem comando)", en: "Loose (no command)" }],
  // Depth-adaptive detail (spec `dashboard-aba-atividade-redesenho`): the
  // pipeline journey for spec-backed (feature) items + the lean note for the
  // single-layer task/bugfix/tactical-fix items.
  ["activity.journey", { pt: "Jornada", en: "Journey" }],
  ["activity.stage.analyze", { pt: "Analyze", en: "Analyze" }],
  ["activity.stage.plan", { pt: "Plan", en: "Plan" }],
  ["activity.stage.execute", { pt: "Execute", en: "Execute" }],
  ["activity.stage.qa", { pt: "QA", en: "QA" }],
  ["activity.stage.close", { pt: "Close", en: "Close" }],
  ["activity.waves", { pt: "Ondas", en: "Waves" }],
  ["activity.waves.empty", { pt: "Sem ondas registradas.", en: "No waves recorded." }],
  ["activity.waveStatus.completed", { pt: "concluída", en: "completed" }],
  ["activity.waveStatus.in_progress", { pt: "em curso", en: "in progress" }],
  ["activity.waveStatus.failed", { pt: "falhou", en: "failed" }],
  ["activity.waveStatus.queued", { pt: "aguardando", en: "queued" }],
  ["activity.quality", { pt: "Qualidade — critérios de aceitação", en: "Quality — acceptance criteria" }],
  ["activity.quality.empty", { pt: "QA ainda não rodou.", en: "QA has not run yet." }],
  ["activity.openSpec", { pt: "Abrir spec (PRD · Spec · Ondas · Qualidade)", en: "Open spec (PRD · Spec · Waves · Quality)" }],
  ["activity.leanNote", { pt: "— ajuste de uma camada: sem ondas, PRD ou QA.", en: "— single-layer change: no waves, PRD or QA." }],
  // At-a-glance chips on a closed spec-backed row.
  ["activity.chip.waves", { pt: "Ondas", en: "Waves" }],
  ["activity.chip.closed", { pt: "Encerrada", en: "Closed" }],
  // Explainer cards above the list.
  ["activity.note.grouped.title", { pt: "Agrupado por tipo", en: "Grouped by type" }],
  ["activity.note.grouped.body", { pt: "feature · ajuste · correção · mudança rápida — revela até o trabalho enxuto que nunca virou spec.", en: "feature · change · fix · quick fix — surfaces even the lean work that never became a spec." }],
  ["activity.note.rail.title", { pt: "Trilho de estágios", en: "Stage rail" }],
  ["activity.note.rail.body", { pt: "Só funcionalidades mostram o caminho Analyze→Close. A ausência dele já diz que o item é enxuto.", en: "Only features show the Analyze→Close path. Its absence signals a lean item." }],
  ["activity.note.spec.title", { pt: "Spec a um toque", en: "Spec in one tap" }],
  ["activity.note.spec.body", { pt: "Ondas, PRD, Spec e QA abrem o drill-in que já existe — nada foi perdido.", en: "Waves, PRD, Spec and QA open the existing drill-in — nothing was lost." }],
  ["common.back", { pt: "Voltar", en: "Back" }],

  // ── Knowledge page ────────────────────────────────────────────────────
  ["knowledge.editorialTitle", { pt: "Conhecimento e atrito", en: "Knowledge and friction" }],
  ["knowledge.editorialSubtitle", { pt: "Padrões, decisões e lições reutilizáveis extraídos das pipelines, separados dos sinais de fricção medidos durante as execuções.", en: "Reusable patterns, decisions and lessons extracted from pipelines, separated from friction signals measured during executions." }],
  ["knowledge.search.placeholder", { pt: "Buscar padrões, convenções, decisões, lições…", en: "Search patterns, conventions, decisions, lessons…" }],
  ["knowledge.search.aria", { pt: "Buscar conhecimento", en: "Search knowledge" }],
  ["knowledge.searchEmpty.title", { pt: "Nenhum resultado para \"{query}\"", en: "No results for \"{query}\"" }],
  ["knowledge.searchEmpty.description", { pt: "Tente um termo mais curto, ou limpe a busca para ver tudo agrupado por tipo.", en: "Try a shorter term, or clear the search to view everything grouped by type." }],
  ["knowledge.section.results", { pt: "Resultados", en: "Results" }],
  ["knowledge.section.patterns.title", { pt: "Padrões e decisões", en: "Patterns and decisions" }],
  ["knowledge.section.patterns.description", { pt: "Conhecimento reutilizável extraído das pipelines: convenções de código, decisões de arquitetura, padrões de nomenclatura e lições. O rótulo CONVENÇÃO aparece só para convenções de código de verdade. Telemetria de fricção é filtrada daqui e aparece na seção Atrito.", en: "Reusable knowledge extracted from pipelines: code conventions, architectural decisions, naming patterns and lessons. The CONVENTION label only shows for genuine code conventions. Friction telemetry is filtered out and appears in the Friction section." }],
  ["knowledge.empty.noPatterns.title", { pt: "Nenhum padrão capturado ainda", en: "No patterns captured yet" }],
  ["knowledge.empty.noPatterns.body.before", { pt: "O Mustard extrai padrões automaticamente ao final de cada pipeline. Rode um ", en: "Mustard extracts patterns automatically at the end of each pipeline. Run a " }],
  ["knowledge.empty.noPatterns.body.or", { pt: " ou ", en: " or " }],
  ["knowledge.empty.noPatterns.body.invoke", { pt: ", ou invoque ", en: ", or invoke " }],
  ["knowledge.empty.noPatterns.body.after", { pt: " para forçar uma extração. Se este workspace tem instalação antiga do Mustard, é normal ver poucas entradas aqui — o resto era telemetria de fricção e foi movido para Atrito.", en: " to force an extraction. If this workspace has an old Mustard install, it is normal to see few entries here — the rest was friction telemetry and was moved to Friction." }],
  ["knowledge.friction.title", { pt: "Atrito", en: "Friction" }],
  ["knowledge.friction.description", { pt: "Sinais de fricção medidos durante as pipelines — não é conhecimento, é diagnóstico. Inclui também telemetria legada que um Mustard antigo gravou no lugar errado e foi filtrada de Padrões. É normal estar quase vazio: atrito medido é raro.", en: "Friction signals measured during pipelines — this is diagnosis, not knowledge. It also includes legacy telemetry an old Mustard wrote in the wrong place and which was filtered out of Patterns. It is normal to be nearly empty: measured friction is rare." }],
  ["knowledge.friction.empty.title", { pt: "Nenhum atrito registrado", en: "No friction recorded" }],
  ["knowledge.friction.empty.description", { pt: "As pipelines deste workspace rodaram sem fricção acima do limite (mais de 2 retries de hook ou mais de 50 chamadas de API por pipeline). Isso é bom — é o estado esperado.", en: "The pipelines in this workspace ran without friction above the threshold (more than 2 hook retries or more than 50 API calls per pipeline). That is good — it is the expected state." }],
  ["knowledge.friction.legacy.label", { pt: "Atrito", en: "Friction" }],
  ["knowledge.friction.legacy.tag", { pt: "Telemetria legada", en: "Legacy telemetry" }],
  ["knowledge.friction.legacy.collapse", { pt: "Mostrar entradas", en: "Show entries" }],
  ["knowledge.friction.legacy.hint", { pt: "Entradas de fricção (heavy-pipeline, high-hook-retry, .metrics) gravadas em knowledge.json por um extractor antigo, sem contadores medidos. Mantidas só para inspeção.", en: "Friction entries (heavy-pipeline, high-hook-retry, .metrics) written into knowledge.json by an old extractor, with no measured counters. Kept for inspection only." }],
  ["knowledge.friction.retries", { pt: "retries", en: "retries" }],
  ["knowledge.friction.retriesTitle", { pt: "Retries de hook medidos nesta pipeline (sandbox/stash/re-prompt — não redespacho de agente).", en: "Hook retries measured in this pipeline (sandbox/stash/re-prompt — not agent redispatch)." }],
  ["knowledge.friction.calls", { pt: "chamadas", en: "calls" }],
  ["knowledge.friction.callsTitle", { pt: "Total de chamadas de API medidas nesta pipeline.", en: "Total API calls measured in this pipeline." }],
  ["knowledge.friction.suggestion", { pt: "Sugestão:", en: "Suggestion:" }],
  ["knowledge.types.entityCluster", { pt: "Cluster de entidade", en: "Entity cluster" }],
  ["knowledge.types.namingPattern", { pt: "Padrão de nomenclatura", en: "Naming pattern" }],
  ["knowledge.types.decision", { pt: "Decisão", en: "Decision" }],
  ["knowledge.types.lesson", { pt: "Lição", en: "Lesson" }],
  ["knowledge.types.convention", { pt: "Convenção", en: "Convention" }],
  ["knowledge.types.pattern", { pt: "Padrão", en: "Pattern" }],

  // ── Commands page ─────────────────────────────────────────────────────
  ["commands.editorialTitle", { pt: "Commands", en: "Commands" }],
  ["commands.editorialSubtitle", { pt: "Catálogo de comandos slash disponíveis no Claude Code com Mustard. Use a busca ou os filtros de categoria para isolar o comando certo.", en: "Catalogue of slash commands available in Claude Code with Mustard. Use the search or category filters to find the right command." }],
  ["commands.search.placeholder", { pt: "Buscar por nome, descrição, categoria…", en: "Search by name, description, category…" }],
  ["commands.filter.all", { pt: "Todos", en: "All" }],
  ["commands.empty.noCatalog.title", { pt: "Nenhum comando catalogado", en: "No commands catalogued" }],
  ["commands.empty.noCatalog.description", { pt: "O catálogo de comandos está vazio.", en: "The command catalogue is empty." }],
  ["commands.empty.noResults.title", { pt: "Sem resultados", en: "No results" }],
  ["commands.empty.noResults.description", { pt: "Nenhum comando para \"{query}\". Ajuste a busca ou troque a categoria.", en: "No command for \"{query}\". Adjust the search or switch category." }],
  ["commands.section.plainExplanation", { pt: "Explicação simples", en: "Plain explanation" }],
  ["commands.section.technicalDetails", { pt: "Detalhes técnicos", en: "Technical details" }],
  ["commands.section.whenToUse", { pt: "Quando usar", en: "When to use" }],
  ["commands.section.whenNotToUse", { pt: "Quando NÃO usar", en: "When NOT to use" }],
  ["commands.section.examples", { pt: "Exemplos", en: "Examples" }],
  ["commands.section.seeAlso", { pt: "Ver também", en: "See also" }],
  ["commands.copy", { pt: "Copiar", en: "Copy" }],

  // ── Spec waves tab ────────────────────────────────────────────────────
  ["specWaves.status.completed", { pt: "concluída", en: "completed" }],
  ["specWaves.status.in_progress", { pt: "em execução", en: "in progress" }],
  ["specWaves.status.failed", { pt: "falhou", en: "failed" }],
  ["specWaves.status.queued", { pt: "aguardando", en: "queued" }],
  ["specWaves.source.event", { pt: "evento", en: "event" }],
  ["specWaves.source.header", { pt: "header", en: "header" }],
  ["specWaves.source.both", { pt: "ambos", en: "both" }],
  ["specWaves.empty", { pt: "Nenhuma onda registrada para esta spec.", en: "No waves recorded for this spec." }],
  ["specWaves.child.openTitle", { pt: "Abrir {spec}", en: "Open {spec}" }],
  ["specWaves.child.durationTitle", { pt: "Duração do filho", en: "Child duration" }],
  ["specWaves.child.source.event", { pt: "Descoberto via evento SQLite spec.link", en: "Discovered via SQLite spec.link event" }],
  ["specWaves.child.source.header", { pt: "Descoberto via header `### Parent:` no markdown", en: "Discovered via `### Parent:` header in markdown" }],
  ["specWaves.child.source.both", { pt: "Presente em evento E header", en: "Present in both event AND header" }],
  ["specWaves.row.openWaveAria", { pt: "Abrir markdown da wave {n}", en: "Open markdown for wave {n}" }],
  ["specWaves.row.collapseAria", { pt: "Colapsar sub-specs", en: "Collapse sub-specs" }],
  ["specWaves.row.expandAria", { pt: "Expandir sub-specs", en: "Expand sub-specs" }],
  ["specWaves.row.collapseTitle", { pt: "Esconder sub-specs desta onda", en: "Hide sub-specs in this wave" }],
  ["specWaves.row.expandTitle", { pt: "Mostrar {count} sub-spec{plural} desta onda", en: "Show {count} sub-spec{plural} in this wave" }],
  ["specWaves.row.subSpecsTitle", { pt: "{count} sub-specs criadas dentro desta onda", en: "{count} sub-specs created in this wave" }],
  ["specWaves.row.startedAt", { pt: "início:", en: "started:" }],
  ["specWaves.row.completedAt", { pt: "fim:", en: "finished:" }],
  ["specWaves.row.durationLabel", { pt: "duração:", en: "duration:" }],
  ["specWaves.row.durationTitle", { pt: "duração total da onda", en: "total wave duration" }],
  ["specWaves.row.fileCountTitle.declared", { pt: "arquivos declarados em `## Arquivos`", en: "files declared in `## Files`" }],
  ["specWaves.row.fileCountTitle.touched", { pt: "arquivos tocados pela onda (eventos tool.use)", en: "files touched by the wave (tool.use events)" }],
  ["specWaves.row.fileSingular", { pt: "arquivo", en: "file" }],
  ["specWaves.row.filePlural", { pt: "arquivos", en: "files" }],
  ["specWaves.row.waveFailed", { pt: "Onda falhou — ver Qualidade / markdown para detalhes do último erro.", en: "Wave failed — see Quality / markdown for details of the last error." }],
  ["specWaves.row.specPrincipal", { pt: "spec principal", en: "main spec" }],
  ["specWaves.row.mainSpecLabel", { pt: "Spec principal", en: "Main spec" }],
  ["specWaves.orphans.label", { pt: "Sem onda correlacionada", en: "No correlated wave" }],
  ["specWaves.orphans.aria", { pt: "Sub-specs sem onda correlacionada", en: "Sub-specs with no correlated wave" }],
  ["specWaves.row.runningBadge", { pt: "EXECUTANDO", en: "RUNNING" }],
  ["specWaves.row.runningBadgeTitle", { pt: "Esta onda está em execução agora", en: "This wave is running right now" }],
  ["specWaves.row.checklistTitle", { pt: "itens do checklist concluídos nesta onda (meta.json + eventos checklist.item.marked)", en: "checklist items done in this wave (meta.json + checklist.item.marked events)" }],
  ["specWaves.row.checklistCount", { pt: "{done}/{total} itens", en: "{done}/{total} items" }],
  ["specWaves.row.checklistDoneOnly", { pt: "itens marcados: {done}", en: "items marked: {done}" }],
  ["specCard.target.waveRunning", { pt: "Onda {n} — {role} em execução", en: "Wave {n} — {role} running" }],
  ["specCard.target.waveRunningNoRole", { pt: "Onda {n} em execução", en: "Wave {n} running" }],
  ["specCard.target.executing", { pt: "Executando", en: "Running" }],
  ["specCard.digest.used", { pt: "digest ✓ · {n} reads antes", en: "digest ✓ · {n} reads before" }],
  ["specCard.digest.notUsed", { pt: "digest ✗ · {n} reads diretos", en: "digest ✗ · {n} direct reads" }],
  ["specCard.digest.usedTitle", { pt: "Adesão ao digest: o agente consultou o digest no ANALYZE; {n} leituras diretas de código antes da primeira consulta", en: "Digest adherence: the agent queried the digest during ANALYZE; {n} direct source reads before the first query" }],
  ["specCard.digest.notUsedTitle", { pt: "Adesão ao digest: nenhuma consulta ao digest registrada; {n} leituras diretas de código na sessão", en: "Digest adherence: no digest query recorded; {n} direct source reads in the session" }],
  ["specCard.digest.absentTitle", { pt: "Sem telemetria de adesão ao digest para esta spec ainda", en: "No digest-adherence telemetry for this spec yet" }],

  // ── Spec track row (Workspace hero list) ──────────────────────────────
  ["specTrack.inProgressAria", { pt: "Em execução", en: "In progress" }],
  ["specTrack.phasesAria", { pt: "Fases da pipeline", en: "Pipeline phases" }],
  ["specTrack.aria", { pt: "Spec {spec}, fase {phase}, status {status}. Clique para expandir.", en: "Spec {spec}, phase {phase}, status {status}. Click to expand." }],
  ["specTrack.waveLabel", { pt: "onda {current}/{total}", en: "wave {current}/{total}" }],
  ["specTrack.waveLabelOnly", { pt: "onda {current}", en: "wave {current}" }],

  // ── Aggregate overview (Portfolio mode) ───────────────────────────────
  ["aggregate.counter.activeSpecs", { pt: "Specs ativas", en: "Active specs" }],
  ["aggregate.counter.executing", { pt: "Em EXECUTE", en: "In EXECUTE" }],
  ["aggregate.counter.completed7d", { pt: "Completed 7d", en: "Completed 7d" }],
  ["aggregate.counter.eventsToday", { pt: "Eventos hoje", en: "Events today" }],
  ["aggregate.roi.title", { pt: "Compensa usar o Mustard?", en: "Is Mustard worth using?" }],
  ["aggregate.roi.intro", { pt: "Comparação contrafactual de tokens: o que de fato foi para o modelo COM o Mustard, contra a estimativa SEM ele. Os tokens poupados são medidos pelo RTK (compressão de saída de comandos) — não é estimativa de preço.", en: "Counterfactual comparison in tokens: what actually went to the model WITH Mustard, against the estimate WITHOUT it. Saved tokens are measured by RTK (command output compression) — not a price estimate." }],
  ["aggregate.roi.noData.before", { pt: "Ainda sem dados de economia. O RTK precisa estar instalado e ter comprimido pelo menos um comando.", en: "No savings data yet. RTK must be installed and must have compressed at least one command." }],
  ["aggregate.roi.noData.runBefore", { pt: "Rode ", en: "Run " }],
  ["aggregate.roi.noData.runAfter", { pt: " para ativar.", en: " to enable it." }],
  ["aggregate.roi.with.eyebrow", { pt: "COM Mustard — foi ao modelo", en: "WITH Mustard — sent to the model" }],
  ["aggregate.roi.with.foot", { pt: "tokens efetivamente enviados", en: "tokens effectively sent" }],
  ["aggregate.roi.without.eyebrow", { pt: "SEM Mustard — estimativa", en: "WITHOUT Mustard — estimate" }],
  ["aggregate.roi.without.foot", { pt: "consumido + poupado pelo RTK", en: "consumed + saved by RTK" }],
  ["aggregate.roi.saved.eyebrow", { pt: "Diferença poupada", en: "Saved difference" }],
  ["aggregate.roi.saved.foot", { pt: "tokens que o Mustard evitou de enviar", en: "tokens Mustard avoided sending" }],
  ["aggregate.roi.footnote.before", { pt: "Custo em USD é medido pela Anthropic API por projeto — veja em ", en: "USD cost is measured by the Anthropic API per project — see " }],
  ["aggregate.roi.footnote.path", { pt: "Telemetria → Economia", en: "Telemetry → Economy" }],
  ["aggregate.roi.footnote.after", { pt: ". O custo agregado da seção abaixo é estimado (tokens × tabela de preço), não cobrado.", en: ". The aggregate cost in the section below is estimated (tokens × price table), not billed." }],
  ["aggregate.consumption.title", { pt: "Consumo & Economia — todos os projetos", en: "Consumption & Savings — all projects" }],
  ["aggregate.kpi.tokensTotal", { pt: "Tokens total", en: "Tokens total" }],
  ["aggregate.kpi.tokensTodayLabel", { pt: "hoje", en: "today" }],
  ["aggregate.kpi.costUsdEstimated", { pt: "Custo USD (estimado)", en: "USD cost (estimated)" }],
  ["aggregate.kpi.costTodaySuffix", { pt: "· tokens × tabela", en: "· tokens × price table" }],
  ["aggregate.kpi.rtkSaved", { pt: "RTK saved", en: "RTK saved" }],
  ["aggregate.kpi.rtkSavedSubEfic", { pt: "{pct} efic. · global · vitalício", en: "{pct} efic. · global · lifetime" }],
  ["aggregate.kpi.rtkSavedSubGlobal", { pt: "global · todos os projetos", en: "global · all projects" }],
  ["aggregate.kpi.rtkCommands", { pt: "RTK commands", en: "RTK commands" }],
  ["aggregate.kpi.rtkNotInstalled", { pt: "rtk não instalado", en: "rtk not installed" }],
  ["aggregate.spark.consumed", { pt: "consumido", en: "consumed" }],
  ["aggregate.spark.rtkSaved", { pt: "RTK saved", en: "RTK saved" }],
  ["aggregate.byModel.title", { pt: "Por modelo (todos os projetos)", en: "By model (all projects)" }],
  ["aggregate.byModel.costEst", { pt: "est.", en: "est." }],
  ["aggregate.byModel.costEstTooltip", { pt: "Custo estimado: o canal de métrica traz tokens por modelo, mas só um custo total — este valor é rateado pela participação de tokens (pct exato). A coluna de tokens é medida.", en: "Estimated cost: the metric channel has per-model tokens but only a total cost — this figure is apportioned by token share (pct is exact). The tokens column is measured." }],
  ["aggregate.byModel.empty", { pt: "Sem consumo por modelo ainda.", en: "No per-model usage yet." }],
  ["aggregate.byProject.title", { pt: "Por projeto (ordenado por custo)", en: "By project (ordered by cost)" }],
  ["aggregate.byProject.col.project", { pt: "Projeto", en: "Project" }],
  ["aggregate.byProject.col.tokens", { pt: "Tokens", en: "Tokens" }],
  ["aggregate.byProject.col.today", { pt: "Hoje", en: "Today" }],
  ["aggregate.byProject.col.cost", { pt: "Custo", en: "Cost" }],
  ["aggregate.byProject.col.lastActivity", { pt: "Última atividade", en: "Last activity" }],
  ["aggregate.activePipelines.title", { pt: "Pipelines ativas", en: "Active pipelines" }],
  ["aggregate.activePipelines.empty", { pt: "Sem pipelines ativas.", en: "No active pipelines." }],
  ["aggregate.recentActivity.title", { pt: "Atividade recente", en: "Recent activity" }],
  ["aggregate.recentActivity.empty", { pt: "Sem eventos recentes.", en: "No recent events." }],

  // ── Phase theme (chips/tooltips across Quality, Activity, Telemetry) ──
  ["phaseTheme.backlog.label", { pt: "Backlog", en: "Backlog" }],
  ["phaseTheme.backlog.detail", { pt: "Pendente de priorização — fora do fluxo ativo", en: "Awaiting prioritisation — outside the active flow" }],
  ["phaseTheme.analyze.label", { pt: "Analisar", en: "Analyze" }],
  ["phaseTheme.analyze.detail", { pt: "Exploração inicial do problema — Grep/Read sem editar", en: "Initial problem exploration — Grep/Read without editing" }],
  ["phaseTheme.plan.label", { pt: "Planejar", en: "Plan" }],
  ["phaseTheme.plan.detail", { pt: "Desenhando a solução — spec/plan, sem tocar código", en: "Designing the solution — spec/plan, without touching code" }],
  ["phaseTheme.execute.label", { pt: "Executar", en: "Execute" }],
  ["phaseTheme.execute.detail", { pt: "Implementando o código — waves rodam aqui", en: "Implementing the code — waves run here" }],
  ["phaseTheme.qa.label", { pt: "QA", en: "QA" }],
  ["phaseTheme.qa.detail", { pt: "Validando AC — script qa-run executando os critérios", en: "Validating AC — qa-run script executing the criteria" }],
  ["phaseTheme.close.label", { pt: "Fechando", en: "Closing" }],
  ["phaseTheme.close.detail", { pt: "Promovendo para completed e sincronizando registros", en: "Promoting to completed and synchronising records" }],
  ["phaseTheme.none.label", { pt: "Sem fase", en: "No phase" }],
  ["phaseTheme.none.detail", { pt: "Sem fase definida ainda", en: "No phase defined yet" }],
  ["eventTheme.fallback.label", { pt: "evento", en: "event" }],
  ["eventTheme.fallback.detail", { pt: "Tipo de evento não rotulado pelo dashboard ainda", en: "Event type not yet labelled by the dashboard" }],
  ["eventTheme.toolUse.detail", { pt: "Agente usou uma ferramenta (Read, Edit, Bash, Grep, etc.)", en: "Agent used a tool (Read, Edit, Bash, Grep, etc.)" }],
  ["eventTheme.pipelinePhase.detail", { pt: "Transição de fase do pipeline (ex: PLAN → EXECUTE)", en: "Pipeline phase transition (e.g. PLAN → EXECUTE)" }],
  ["eventTheme.qaResult.detail", { pt: "Resultado do QA — overall pass/fail/skip dos AC", en: "QA result — overall pass/fail/skip of the AC" }],
  ["eventTheme.agentStart.detail", { pt: "Agente iniciado via Task dispatch", en: "Agent started via Task dispatch" }],
  ["eventTheme.agentStop.detail", { pt: "Agente encerrou e retornou resumo", en: "Agent finished and returned a summary" }],
  ["eventTheme.sessionStart.detail", { pt: "Sessão Claude Code iniciada", en: "Claude Code session started" }],
  ["eventTheme.specStart.detail", { pt: "Pipeline de spec iniciada", en: "Spec pipeline started" }],
  ["eventTheme.specComplete.detail", { pt: "Pipeline de spec finalizada", en: "Spec pipeline completed" }],
  ["eventTheme.dispatchFailure.detail", { pt: "Falha no dispatch — geralmente overload/rate-limit do modelo", en: "Dispatch failure — usually model overload/rate-limit" }],
  ["eventTheme.retryAttempt.detail", { pt: "Tentativa de fix-loop após review/QA falhar", en: "Fix-loop attempt after review/QA failed" }],
  ["eventTheme.analyzeDigestUsed.detail", { pt: "Digest do scan consultado durante a pesquisa (marcador de adesão)", en: "Scan digest queried during research (adherence marker)" }],
  ["eventTheme.analyzeDigestSummary.detail", { pt: "Resumo de adesão ao digest — usou digest? quantos reads diretos antes?", en: "Digest adherence summary — was the digest used? how many direct reads before?" }],
  ["eventTheme.decision.detail", { pt: "Decisão arquitetural registrada durante a pipeline", en: "Architectural decision recorded during the pipeline" }],
  ["eventTheme.finding.detail", { pt: "Achado/observação registrado pelo agente", en: "Finding/observation recorded by the agent" }],
  ["eventTheme.lesson.detail", { pt: "Aprendizado capturado pra knowledge base", en: "Lesson captured for the knowledge base" }],

  // ── Execution trace (tool event rows) ─────────────────────────────────
  // Write/create has `file_after` but no `file_before` snapshot, so we show
  // the written content as a code view instead of a diff.
  ["trace.tool.writtenContent", { pt: "Conteúdo escrito", en: "Written content" }],
  ["trace.tool.error", { pt: "Comando falhou", en: "Command failed" }],
]);

/**
 * Read the Preferences-controlled language via a zustand selector on the
 * shared `useStore`. Zustand handles subscription/re-render internally, so
 * this hook stays zero-context — any component can call `useTranslate()` /
 * `useT()` without wrapping the tree in a Provider.
 */
function useLang(): Lang {
  // Selector pattern — single field of the store, mirroring the guardrail in
  // `apps/dashboard/CLAUDE.md` ("Select zustand fields via slices").
  return useStore((s) => s.language);
}

/**
 * Resolve a key against the dictionary for an explicit language. Falls back
 * through: active language → PT → EN → caller fallback → key.
 */
function resolve(key: string, lang: Lang, fallback?: string): string {
  const row = DICTIONARY.get(key);
  if (!row) return fallback ?? key;
  const k = dictKey(lang);
  return row[k] ?? row.pt ?? row.en ?? fallback ?? key;
}

/**
 * React hook used by Wave 8 components and the W2 audit. Returns a
 * `t(key, fallback?)` function bound to the current `Preferences.language`
 * value — components re-render when the language slice changes.
 */
export function useTranslate(): (key: string, fallback?: string) => string {
  const lang = useLang();
  return (key, fallback) => resolve(key, lang, fallback);
}

/**
 * Hook alias matching the W2 spec naming (`useT`). Same semantics as
 * `useTranslate` — kept as a parallel export so new call sites can use the
 * shorter name without breaking the older `useTranslate` consumers.
 */
export function useT(): (key: string, fallback?: string) => string {
  return useTranslate();
}

/**
 * Imperative variant for callers outside a React component (e.g. building a
 * label inside a non-hook utility, toast bodies, route-label maps). Reads the
 * latest language synchronously from the zustand store. NOT reactive on its
 * own — pair with the hook in render paths.
 */
export function translate(key: string, fallback?: string): string {
  const lang = useStore.getState().language;
  return resolve(key, lang, fallback);
}

/**
 * Canonical `t(key)` export named by the W2 spec. Same behavior as
 * `translate()` — alias kept for spec parity and to give the broader UI a
 * single, short, idiomatic name to reach for.
 */
export function t(key: string, fallback?: string): string {
  return translate(key, fallback);
}
