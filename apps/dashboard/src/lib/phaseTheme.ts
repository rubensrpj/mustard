/**
 * Shared visual theme for the 5 canonical Mustard pipeline phases.
 *
 * Used across Quality, Activity, and Telemetry pages so the same phase
 * always reads as the same color — ANALYZE is always sky, EXECUTE is always
 * emerald, etc. Consistency lets the user build mental association after a
 * few minutes of using the dashboard.
 *
 * Hue choices:
 *   sky     — exploration, learning, looking around
 *   amber   — planning, caution, deliberation
 *   emerald — action, doing, execution
 *   violet  — verification, judgment, gating
 *   zinc    — done, archived, neutral
 */

export type PhaseTheme = {
  /** Friendly Portuguese label */
  label: string;
  /** One-line description for tooltips and inline hints */
  detail: string;
  /** Tailwind text class for chip text */
  text: string;
  /** Tailwind translucent background for chip body */
  bg: string;
  /** Tailwind border class for chip ring */
  border: string;
  /** Solid background class for left-edge accent stripes */
  stripe: string;
};

/* Notion tag-chip style — solid pale tint + deep saturated text in light,
   translucent in dark. Works as a first-class chip in both themes. */
export const PHASE_THEME: Record<string, PhaseTheme> = {
  BACKLOG: {
    label: "Backlog",
    detail: "Pendente de priorização — fora do fluxo ativo",
    text: "text-slate-600 dark:text-slate-300",
    bg: "bg-slate-100 dark:bg-slate-500/10",
    border: "border-slate-200 dark:border-slate-500/25",
    stripe: "bg-slate-500/60",
  },
  ANALYZE: {
    label: "Analisar",
    detail: "Exploração inicial do problema — Grep/Read sem editar",
    text: "text-sky-700 dark:text-sky-300",
    bg: "bg-sky-100 dark:bg-sky-500/10",
    border: "border-sky-200 dark:border-sky-500/25",
    stripe: "bg-sky-500/60",
  },
  PLAN: {
    label: "Planejar",
    detail: "Desenhando a solução — spec/plan, sem tocar código",
    text: "text-amber-700 dark:text-amber-300",
    bg: "bg-amber-100 dark:bg-amber-500/10",
    border: "border-amber-200 dark:border-amber-500/25",
    stripe: "bg-amber-500/60",
  },
  EXECUTE: {
    label: "Executar",
    detail: "Implementando o código — waves rodam aqui",
    text: "text-emerald-700 dark:text-emerald-300",
    bg: "bg-emerald-100 dark:bg-emerald-500/10",
    border: "border-emerald-200 dark:border-emerald-500/25",
    stripe: "bg-emerald-500/60",
  },
  QA: {
    label: "QA",
    detail: "Validando AC — script `qa-run.js` executando os critérios",
    text: "text-violet-700 dark:text-violet-300",
    bg: "bg-violet-100 dark:bg-violet-500/10",
    border: "border-violet-200 dark:border-violet-500/25",
    stripe: "bg-violet-500/60",
  },
  CLOSE: {
    label: "Fechando",
    detail: "Promovendo para completed e sincronizando registros",
    text: "text-zinc-600 dark:text-zinc-300",
    bg: "bg-zinc-100 dark:bg-zinc-500/10",
    border: "border-zinc-200 dark:border-zinc-500/25",
    stripe: "bg-zinc-500/60",
  },
  "—": {
    label: "Sem fase",
    detail: "Sem fase definida ainda",
    text: "text-muted-foreground",
    bg: "bg-muted/50 dark:bg-muted/20",
    border: "border-border",
    stripe: "bg-muted",
  },
};

export const PHASE_ORDER: string[] = ["BACKLOG", "ANALYZE", "PLAN", "EXECUTE", "QA", "CLOSE", "—"];

export function phaseTheme(phase: string | null | undefined): PhaseTheme {
  const key = (phase ?? "").toUpperCase().trim() || "—";
  return PHASE_THEME[key] ?? PHASE_THEME["—"];
}

/**
 * Event-type theme. Different concept from phase — events tag *what* happened
 * (tool was used, agent started, QA returned), not *where* in the pipeline.
 * Different hue family from phases (rose/cyan/lime/fuchsia) so the two
 * categories never blur visually.
 */
export type EventTheme = {
  label: string;
  detail: string;
  text: string;
  bg: string;
  border: string;
};

const EVENT_THEME: Record<string, EventTheme> = {
  "tool.use": {
    label: "tool",
    detail: "Agente usou uma ferramenta (Read, Edit, Bash, Grep, etc.)",
    text: "text-zinc-600 dark:text-zinc-300",
    bg: "bg-zinc-100 dark:bg-zinc-500/10",
    border: "border-zinc-200 dark:border-zinc-500/25",
  },
  "pipeline.phase": {
    label: "phase",
    detail: "Transição de fase do pipeline (ex: PLAN → EXECUTE)",
    text: "text-amber-700 dark:text-amber-300",
    bg: "bg-amber-100 dark:bg-amber-500/10",
    border: "border-amber-200 dark:border-amber-500/25",
  },
  "qa.result": {
    label: "qa",
    detail: "Resultado do QA — overall pass/fail/skip dos AC",
    text: "text-violet-700 dark:text-violet-300",
    bg: "bg-violet-100 dark:bg-violet-500/10",
    border: "border-violet-200 dark:border-violet-500/25",
  },
  "agent.start": {
    label: "agent ▶",
    detail: "Agente iniciado via Task dispatch",
    text: "text-emerald-700 dark:text-emerald-300",
    bg: "bg-emerald-100 dark:bg-emerald-500/10",
    border: "border-emerald-200 dark:border-emerald-500/25",
  },
  "agent.stop": {
    label: "agent ■",
    detail: "Agente encerrou e retornou resumo",
    text: "text-emerald-600/80 dark:text-emerald-300/70",
    bg: "bg-emerald-50 dark:bg-emerald-500/5",
    border: "border-emerald-200/70 dark:border-emerald-500/20",
  },
  "session.start": {
    label: "session",
    detail: "Sessão Claude Code iniciada",
    text: "text-sky-700 dark:text-sky-300",
    bg: "bg-sky-100 dark:bg-sky-500/10",
    border: "border-sky-200 dark:border-sky-500/25",
  },
  "spec.start": {
    label: "spec ▶",
    detail: "Pipeline de spec iniciada",
    text: "text-sky-700 dark:text-sky-300",
    bg: "bg-sky-100 dark:bg-sky-500/10",
    border: "border-sky-200 dark:border-sky-500/25",
  },
  "spec.complete": {
    label: "spec ✓",
    detail: "Pipeline de spec finalizada",
    text: "text-emerald-700 dark:text-emerald-300",
    bg: "bg-emerald-100 dark:bg-emerald-500/10",
    border: "border-emerald-200 dark:border-emerald-500/25",
  },
  "dispatch.failure": {
    label: "fail",
    detail: "Falha no dispatch — geralmente overload/rate-limit do modelo",
    text: "text-rose-700 dark:text-rose-300",
    bg: "bg-rose-100 dark:bg-rose-500/10",
    border: "border-rose-200 dark:border-rose-500/25",
  },
  "retry.attempt": {
    label: "retry",
    detail: "Tentativa de fix-loop após review/QA falhar",
    text: "text-amber-700 dark:text-amber-300",
    bg: "bg-amber-100 dark:bg-amber-500/10",
    border: "border-amber-200 dark:border-amber-500/25",
  },
  decision: {
    label: "decision",
    detail: "Decisão arquitetural registrada durante a pipeline",
    text: "text-fuchsia-700 dark:text-fuchsia-300",
    bg: "bg-fuchsia-100 dark:bg-fuchsia-500/10",
    border: "border-fuchsia-200 dark:border-fuchsia-500/25",
  },
  finding: {
    label: "finding",
    detail: "Achado/observação registrado pelo agente",
    text: "text-fuchsia-700 dark:text-fuchsia-300",
    bg: "bg-fuchsia-100 dark:bg-fuchsia-500/10",
    border: "border-fuchsia-200 dark:border-fuchsia-500/25",
  },
  lesson: {
    label: "lesson",
    detail: "Aprendizado capturado pra knowledge base",
    text: "text-fuchsia-700 dark:text-fuchsia-300",
    bg: "bg-fuchsia-100 dark:bg-fuchsia-500/10",
    border: "border-fuchsia-200 dark:border-fuchsia-500/25",
  },
};

const FALLBACK_EVENT_THEME: EventTheme = {
  label: "evento",
  detail: "Tipo de evento não rotulado pelo dashboard ainda",
  text: "text-muted-foreground",
  bg: "bg-muted/50 dark:bg-muted/20",
  border: "border-border",
};

export function eventTheme(eventType: string): EventTheme {
  return EVENT_THEME[eventType] ?? FALLBACK_EVENT_THEME;
}

/** Strip the date prefix from a spec name for display (`2026-05-14-foo` → `foo`). */
export function shortSpecName(name: string): string {
  return name.replace(/^\d{4}-\d{2}-\d{2}-/, "");
}
