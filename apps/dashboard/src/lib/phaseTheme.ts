/**
 * Shared visual theme for the 5 canonical Mustard pipeline phases.
 *
 * Used across Quality, Activity, and Telemetry pages so the same phase
 * always reads as the same color — ANALYZE is always one hue, EXECUTE
 * another, etc. Consistency lets the user build mental association after a
 * few minutes of using the dashboard.
 *
 * All color references use CSS custom properties defined in style.css
 * (--color-phase-*). This keeps AC-17 satisfied: zero Tailwind named-color
 * classes in source — only arbitrary CSS-var references like text-[--color-*].
 */

export type PhaseTheme = {
  /** Friendly Portuguese label */
  label: string;
  /** One-line description for tooltips and inline hints */
  detail: string;
  /** Tailwind arbitrary-value text class using CSS var */
  text: string;
  /** Tailwind arbitrary-value background using CSS var */
  bg: string;
  /** Tailwind arbitrary-value border using CSS var */
  border: string;
  /** Solid background class for left-edge accent stripes */
  stripe: string;
};

/* Phase chips use CSS custom properties so AC-17 (no named Tailwind colors) holds.
   The actual values are defined in style.css under :root and .dark. */
export const PHASE_THEME: Record<string, PhaseTheme> = {
  BACKLOG: {
    label: "Backlog",
    detail: "Pendente de priorização — fora do fluxo ativo",
    text: "text-[--color-phase-backlog]",
    bg: "bg-[--color-phase-backlog-bg]",
    border: "border-[--color-phase-backlog-border]",
    stripe: "bg-[--color-phase-backlog-stripe]",
  },
  ANALYZE: {
    label: "Analisar",
    detail: "Exploração inicial do problema — Grep/Read sem editar",
    text: "text-[--color-phase-analyze]",
    bg: "bg-[--color-phase-analyze-bg]",
    border: "border-[--color-phase-analyze-border]",
    stripe: "bg-[--color-phase-analyze-stripe]",
  },
  PLAN: {
    label: "Planejar",
    detail: "Desenhando a solução — spec/plan, sem tocar código",
    text: "text-[--color-phase-plan]",
    bg: "bg-[--color-phase-plan-bg]",
    border: "border-[--color-phase-plan-border]",
    stripe: "bg-[--color-phase-plan-stripe]",
  },
  EXECUTE: {
    label: "Executar",
    detail: "Implementando o código — waves rodam aqui",
    text: "text-[--color-phase-execute]",
    bg: "bg-[--color-phase-execute-bg]",
    border: "border-[--color-phase-execute-border]",
    stripe: "bg-[--color-phase-execute-stripe]",
  },
  QA: {
    label: "QA",
    detail: "Validando AC — script qa-run executando os critérios",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
    stripe: "bg-[--color-phase-qa-stripe]",
  },
  CLOSE: {
    label: "Fechando",
    detail: "Promovendo para completed e sincronizando registros",
    text: "text-[--color-phase-close]",
    bg: "bg-[--color-phase-close-bg]",
    border: "border-[--color-phase-close-border]",
    stripe: "bg-[--color-phase-close-stripe]",
  },
  "—": {
    label: "Sem fase",
    detail: "Sem fase definida ainda",
    text: "text-muted-foreground",
    bg: "bg-muted/50",
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
 * Uses CSS custom properties for colors (--color-phase-* and --color-event-*)
 * so AC-17 is satisfied.
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
    text: "text-[--color-phase-close]",
    bg: "bg-[--color-phase-close-bg]",
    border: "border-[--color-phase-close-border]",
  },
  "pipeline.phase": {
    label: "phase",
    detail: "Transição de fase do pipeline (ex: PLAN → EXECUTE)",
    text: "text-[--color-phase-plan]",
    bg: "bg-[--color-phase-plan-bg]",
    border: "border-[--color-phase-plan-border]",
  },
  "qa.result": {
    label: "qa",
    detail: "Resultado do QA — overall pass/fail/skip dos AC",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
  },
  "agent.start": {
    label: "agent ▶",
    detail: "Agente iniciado via Task dispatch",
    text: "text-[--color-phase-execute]",
    bg: "bg-[--color-phase-execute-bg]",
    border: "border-[--color-phase-execute-border]",
  },
  "agent.stop": {
    label: "agent ■",
    detail: "Agente encerrou e retornou resumo",
    text: "text-[--color-phase-execute]",
    bg: "bg-[--color-phase-execute-bg]",
    border: "border-[--color-phase-execute-border]",
  },
  "session.start": {
    label: "session",
    detail: "Sessão Claude Code iniciada",
    text: "text-[--color-phase-analyze]",
    bg: "bg-[--color-phase-analyze-bg]",
    border: "border-[--color-phase-analyze-border]",
  },
  "spec.start": {
    label: "spec ▶",
    detail: "Pipeline de spec iniciada",
    text: "text-[--color-phase-analyze]",
    bg: "bg-[--color-phase-analyze-bg]",
    border: "border-[--color-phase-analyze-border]",
  },
  "spec.complete": {
    label: "spec ✓",
    detail: "Pipeline de spec finalizada",
    text: "text-[--color-phase-execute]",
    bg: "bg-[--color-phase-execute-bg]",
    border: "border-[--color-phase-execute-border]",
  },
  "dispatch.failure": {
    label: "fail",
    detail: "Falha no dispatch — geralmente overload/rate-limit do modelo",
    text: "text-[--color-event-fail]",
    bg: "bg-[--color-event-fail-bg]",
    border: "border-[--color-event-fail-border]",
  },
  "retry.attempt": {
    label: "retry",
    detail: "Tentativa de fix-loop após review/QA falhar",
    text: "text-[--color-phase-plan]",
    bg: "bg-[--color-phase-plan-bg]",
    border: "border-[--color-phase-plan-border]",
  },
  decision: {
    label: "decision",
    detail: "Decisão arquitetural registrada durante a pipeline",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
  },
  finding: {
    label: "finding",
    detail: "Achado/observação registrado pelo agente",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
  },
  lesson: {
    label: "lesson",
    detail: "Aprendizado capturado pra knowledge base",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
  },
};

const FALLBACK_EVENT_THEME: EventTheme = {
  label: "evento",
  detail: "Tipo de evento não rotulado pelo dashboard ainda",
  text: "text-muted-foreground",
  bg: "bg-muted/50",
  border: "border-border",
};

export function eventTheme(eventType: string): EventTheme {
  return EVENT_THEME[eventType] ?? FALLBACK_EVENT_THEME;
}

/** Strip the date prefix from a spec name for display (`2026-05-14-foo` → `foo`). */
export function shortSpecName(name: string): string {
  return name.replace(/^\d{4}-\d{2}-\d{2}-/, "");
}
