// Lightweight in-house i18n provider used across the dashboard.
//
// Background — two i18n surfaces coexist in this repo:
//
//   1. `src/i18n.ts` — i18next instance, namespace `common`. Older pages
//      (Sidebar projects/menu, Settings, Preferences, projects toasts) consume
//      it via `useTranslation()` from `react-i18next`.
//   2. THIS module — a flat `Map<string, Record<'pt'|'en', string>>` bound to
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

export type Lang = "pt" | "en";

/** A single translation row keyed by language. `fallback?` lets a caller
 *  override a missing entry without polluting the dictionary. */
export type TranslationRow = Record<Lang, string>;

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
  ["sidebar.prd", { pt: "PRD", en: "PRD" }],
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
  ["route.specs.groups.close", { pt: "Fechadas", en: "Closed" }],
  ["route.specs.groups.cancelled", { pt: "Canceladas", en: "Cancelled" }],
  ["route.specs.groups.abandoned", { pt: "Abandonadas", en: "Abandoned" }],
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
  return row[lang] ?? row.pt ?? row.en ?? fallback ?? key;
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
