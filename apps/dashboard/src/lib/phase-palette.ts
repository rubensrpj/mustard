/**
 * Wave 4 (spec `2026-05-21-dashboard-spec-tabs-polish`): canonical color
 * palette for the 5+1 Mustard pipeline phases. The map is the single source
 * of truth for the `<PipelineTimeline>` chips and the `<PhaseChip>` pill —
 * same hue everywhere so the user builds visual association.
 *
 * Each entry is a bag of Tailwind 4 utility classes (background tint, text
 * color, border, and a ring color used to highlight the active phase). The
 * `execute` row maps to the canonical mustard accent (`--color-accent-mustard`)
 * which already drives "in progress" affordances across the app.
 *
 * Lower-case keys mirror the harness `pipeline.phase` event value
 * (`analyze`, `plan`, …); `phaseColor()` normalizes case so callers can pass
 * either `"ANALYZE"` or `"analyze"` without thinking.
 */
export const PHASE_COLORS: Record<
  string,
  { bg: string; text: string; border: string; ring: string }
> = {
  // Wave (follow-up `dashboard-spec-list-tree-polish`): the palette was bumped
  // to more saturated tints + brighter text (-300 over -400) + stronger borders
  // so the phase chips read as vivid/alive rather than washed-out. Same hue
  // families as before (sky/violet/green/amber/emerald/slate) — only the
  // intensity moved. `bg` → /20-/25, `text` → -300, `border` → /50, `ring`
  // → /60 so the active station's ring is unmistakable.
  analyze: {
    bg: "bg-sky-500/20",
    text: "text-sky-300",
    border: "border-sky-500/50",
    ring: "ring-sky-400/60",
  },
  plan: {
    bg: "bg-violet-500/20",
    text: "text-violet-300",
    border: "border-violet-500/50",
    ring: "ring-violet-400/60",
  },
  // EXECUTE — the most energetic moment of the run. Brightest green tint so the
  // active station pops the hardest.
  execute: {
    bg: "bg-green-500/25",
    text: "text-green-300",
    border: "border-green-500/60",
    ring: "ring-green-400/70",
  },
  // REVIEW stays amber — separates the two verification phases (review + qa)
  // from each other and from the greens around them.
  review: {
    bg: "bg-amber-500/20",
    text: "text-amber-300",
    border: "border-amber-500/50",
    ring: "ring-amber-400/60",
  },
  qa: {
    bg: "bg-emerald-500/20",
    text: "text-emerald-300",
    border: "border-emerald-500/50",
    ring: "ring-emerald-400/60",
  },
  close: {
    bg: "bg-slate-500/20",
    text: "text-slate-300",
    border: "border-slate-500/50",
    ring: "ring-slate-400/60",
  },
};

export function phaseColor(phase: string) {
  return PHASE_COLORS[phase.toLowerCase()] ?? PHASE_COLORS.close;
}
