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
 *
 * Localization
 * ------------
 * `label` / `detail` strings are RESOLVED THROUGH `t(key)` against the
 * dictionary in `lib/i18n.ts`. Each entry below carries i18n keys so
 * `phaseTheme()` returns translated strings every time it is called
 * (`t()` reads the current language synchronously from the zustand store).
 */
import { t } from "@/lib/i18n";

export type PhaseTheme = {
  /** Friendly localized label */
  label: string;
  /** One-line localized description for tooltips and inline hints */
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
   The actual values are defined in style.css under :root and .dark.
   Static palette (colors only) — labels/details are pulled from i18n at read time. */
type PhasePalette = Omit<PhaseTheme, "label" | "detail">;

const PHASE_PALETTE: Record<string, PhasePalette> = {
  BACKLOG: {
    text: "text-[--color-phase-backlog]",
    bg: "bg-[--color-phase-backlog-bg]",
    border: "border-[--color-phase-backlog-border]",
    stripe: "bg-[--color-phase-backlog-stripe]",
  },
  ANALYZE: {
    text: "text-[--color-phase-analyze]",
    bg: "bg-[--color-phase-analyze-bg]",
    border: "border-[--color-phase-analyze-border]",
    stripe: "bg-[--color-phase-analyze-stripe]",
  },
  PLAN: {
    text: "text-[--color-phase-plan]",
    bg: "bg-[--color-phase-plan-bg]",
    border: "border-[--color-phase-plan-border]",
    stripe: "bg-[--color-phase-plan-stripe]",
  },
  EXECUTE: {
    text: "text-[--color-phase-execute]",
    bg: "bg-[--color-phase-execute-bg]",
    border: "border-[--color-phase-execute-border]",
    stripe: "bg-[--color-phase-execute-stripe]",
  },
  QA: {
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
    stripe: "bg-[--color-phase-qa-stripe]",
  },
  CLOSE: {
    text: "text-[--color-phase-close]",
    bg: "bg-[--color-phase-close-bg]",
    border: "border-[--color-phase-close-border]",
    stripe: "bg-[--color-phase-close-stripe]",
  },
  "—": {
    text: "text-muted-foreground",
    bg: "bg-muted/50",
    border: "border-border",
    stripe: "bg-muted",
  },
};

/** i18n keys for phase label/detail per canonical phase. */
const PHASE_I18N: Record<string, { label: string; detail: string }> = {
  BACKLOG: { label: "phaseTheme.backlog.label", detail: "phaseTheme.backlog.detail" },
  ANALYZE: { label: "phaseTheme.analyze.label", detail: "phaseTheme.analyze.detail" },
  PLAN: { label: "phaseTheme.plan.label", detail: "phaseTheme.plan.detail" },
  EXECUTE: { label: "phaseTheme.execute.label", detail: "phaseTheme.execute.detail" },
  QA: { label: "phaseTheme.qa.label", detail: "phaseTheme.qa.detail" },
  CLOSE: { label: "phaseTheme.close.label", detail: "phaseTheme.close.detail" },
  "—": { label: "phaseTheme.none.label", detail: "phaseTheme.none.detail" },
};

export const PHASE_ORDER: string[] = ["BACKLOG", "ANALYZE", "PLAN", "EXECUTE", "QA", "CLOSE", "—"];

export function phaseTheme(phase: string | null | undefined): PhaseTheme {
  const key = (phase ?? "").toUpperCase().trim() || "—";
  const palette = PHASE_PALETTE[key] ?? PHASE_PALETTE["—"];
  const i18n = PHASE_I18N[key] ?? PHASE_I18N["—"];
  return {
    ...palette,
    label: t(i18n.label),
    detail: t(i18n.detail),
  };
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

type EventPalette = Omit<EventTheme, "label" | "detail"> & {
  /** Glyph label kept as-is (locale-agnostic icon-style label). */
  label: string;
  /** i18n key for the `detail` description. */
  detailKey: string;
};

const EVENT_THEME: Record<string, EventPalette> = {
  "tool.use": {
    label: "tool",
    detailKey: "eventTheme.toolUse.detail",
    text: "text-[--color-phase-close]",
    bg: "bg-[--color-phase-close-bg]",
    border: "border-[--color-phase-close-border]",
  },
  "pipeline.phase": {
    label: "phase",
    detailKey: "eventTheme.pipelinePhase.detail",
    text: "text-[--color-phase-plan]",
    bg: "bg-[--color-phase-plan-bg]",
    border: "border-[--color-phase-plan-border]",
  },
  "qa.result": {
    label: "qa",
    detailKey: "eventTheme.qaResult.detail",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
  },
  "agent.start": {
    label: "agent ▶",
    detailKey: "eventTheme.agentStart.detail",
    text: "text-[--color-phase-execute]",
    bg: "bg-[--color-phase-execute-bg]",
    border: "border-[--color-phase-execute-border]",
  },
  "agent.stop": {
    label: "agent ■",
    detailKey: "eventTheme.agentStop.detail",
    text: "text-[--color-phase-execute]",
    bg: "bg-[--color-phase-execute-bg]",
    border: "border-[--color-phase-execute-border]",
  },
  "session.start": {
    label: "session",
    detailKey: "eventTheme.sessionStart.detail",
    text: "text-[--color-phase-analyze]",
    bg: "bg-[--color-phase-analyze-bg]",
    border: "border-[--color-phase-analyze-border]",
  },
  "spec.start": {
    label: "spec ▶",
    detailKey: "eventTheme.specStart.detail",
    text: "text-[--color-phase-analyze]",
    bg: "bg-[--color-phase-analyze-bg]",
    border: "border-[--color-phase-analyze-border]",
  },
  "spec.complete": {
    label: "spec ✓",
    detailKey: "eventTheme.specComplete.detail",
    text: "text-[--color-phase-execute]",
    bg: "bg-[--color-phase-execute-bg]",
    border: "border-[--color-phase-execute-border]",
  },
  "dispatch.failure": {
    label: "fail",
    detailKey: "eventTheme.dispatchFailure.detail",
    text: "text-[--color-event-fail]",
    bg: "bg-[--color-event-fail-bg]",
    border: "border-[--color-event-fail-border]",
  },
  "retry.attempt": {
    label: "retry",
    detailKey: "eventTheme.retryAttempt.detail",
    text: "text-[--color-phase-plan]",
    bg: "bg-[--color-phase-plan-bg]",
    border: "border-[--color-phase-plan-border]",
  },
  decision: {
    label: "decision",
    detailKey: "eventTheme.decision.detail",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
  },
  finding: {
    label: "finding",
    detailKey: "eventTheme.finding.detail",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
  },
  lesson: {
    label: "lesson",
    detailKey: "eventTheme.lesson.detail",
    text: "text-[--color-phase-qa]",
    bg: "bg-[--color-phase-qa-bg]",
    border: "border-[--color-phase-qa-border]",
  },
};

const FALLBACK_EVENT_PALETTE: EventPalette = {
  label: "event",
  detailKey: "eventTheme.fallback.detail",
  text: "text-muted-foreground",
  bg: "bg-muted/50",
  border: "border-border",
};

export function eventTheme(eventType: string): EventTheme {
  const palette = EVENT_THEME[eventType] ?? FALLBACK_EVENT_PALETTE;
  const label = palette === FALLBACK_EVENT_PALETTE ? t("eventTheme.fallback.label") : palette.label;
  return {
    label,
    detail: t(palette.detailKey),
    text: palette.text,
    bg: palette.bg,
    border: palette.border,
  };
}

/** Strip the date prefix from a spec name for display (`2026-05-14-foo` → `foo`). */
export function shortSpecName(name: string): string {
  return name.replace(/^\d{4}-\d{2}-\d{2}-/, "");
}
