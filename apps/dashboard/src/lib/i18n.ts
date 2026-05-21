// Lightweight in-house i18n provider for the Visão Geral page.
//
// Why caseiro instead of reusing the existing `src/i18n.ts` (i18next):
//
// 1. Wave 8 (spec `2026-05-20-economia-moat-unification/wave-8-visao-geral-revamp`)
//    asks for the Overview labels to be lazily migrable. A flat
//    `Map<string, Record<'pt'|'en', string>>` keeps the dictionary inspectable
//    and grep-able from the React side without dragging i18next options into
//    each new component.
// 2. The provider binds straight to the existing **Preferences** slice on the
//    zustand store (`useStore((s) => s.language)`) so the only persisted
//    setting that controls language lives in one place. There is no separate
//    `usePreferences` slice — Preferences is the page that mutates the slice
//    via `setLanguage`; the binding name in this module mirrors that mental
//    model.
// 3. Calling `setLanguage` on the zustand store already syncs `i18next` (see
//    `src/lib/store.ts::setLanguage`), so legacy pages that still consume
//    `useTranslation()` from `react-i18next` stay correct — this module just
//    gives the Overview page a smaller, dependency-free surface.

import { useStore } from "@/lib/store";

export type Lang = "pt" | "en";

/** A single translation row keyed by language. `fallback?` lets a caller
 *  override a missing entry without polluting the dictionary. */
export type TranslationRow = Record<Lang, string>;

/**
 * Flat dictionary for the Visão Geral page. Keep entries grouped by surface
 * (`workspace.*`, `period.*`) so future migrations stay readable. New keys go
 * in this map — components consume them via `useTranslate()`.
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

  // Section titles.
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

  // Hero empty / labels.
  ["hero.empty", { pt: "Nenhum pipeline ativo", en: "No active pipeline" }],
  ["hero.emptyHint", { pt: "Inicie uma pipeline para vê-la aqui.", en: "Start a pipeline to see it here." }],
  ["hero.duration", { pt: "duração", en: "duration" }],
  ["hero.tokens", { pt: "tokens", en: "tokens" }],
]);

/**
 * Subscribe to the Preferences-controlled language. `useSyncExternalStore`
 * keeps this provider zero-context: any component can call `useTranslate()`
 * without wrapping the tree.
 */
function useLang(): Lang {
  // Selector pattern — single field of the store, mirroring the guardrail in
  // `apps/dashboard/CLAUDE.md` ("Select zustand fields via slices").
  return useStore((s) => s.language);
}

/**
 * React hook used by Wave 8 components. Returns a `t(key, fallback?)` function
 * bound to the current `Preferences.language` value.
 *
 * Resolution order: dictionary entry for the active language → caller-supplied
 * fallback → the key itself (so missing entries surface visibly in dev rather
 * than collapsing to an empty string).
 */
export function useTranslate(): (key: string, fallback?: string) => string {
  const lang = useLang();
  return (key, fallback) => {
    const row = DICTIONARY.get(key);
    if (row) return row[lang];
    return fallback ?? key;
  };
}

/**
 * Imperative variant for callers outside a React component (e.g. building a
 * label inside a non-hook utility). Reads the latest language synchronously
 * from the zustand store.
 */
export function translate(key: string, fallback?: string): string {
  const lang = useStore.getState().language;
  const row = DICTIONARY.get(key);
  if (row) return row[lang];
  return fallback ?? key;
}
