import { cn } from "@/lib/utils";

/**
 * Shared StatusPill + status maps for spec list/card UIs.
 * Single source of truth — both SpecCard and SpecChildrenTab consume.
 *
 * Extracted by spec `2026-05-21-extract-statuspill` from the verbatim
 * duplicates that had appeared in both files. The SpecChildrenTab variant
 * (newer) is the authoritative shape — includes `tabular-nums` on the pill
 * so future numeric statuses align cleanly. Existing status labels are all
 * non-numeric text, so the addition is a no-op for current renders.
 */

// Map a typed `mustard-specsdb::SpecStatus` (serialized as kebab-case) to a
// short human-readable label. Renders honest empty state ("—") when the spec
// has no harness events yet, instead of the old grey "UNKNOWN" badge.
export const STATUS_LABELS: Record<string, string> = {
  "no-events":       "—",
  planning:          "planejamento",
  implementing:      "ativa",
  reviewing:         "review",
  qa:                "QA",
  "awaiting-close":  "aguard. fechar",
  "closed-followup": "follow-up",
  completed:         "concluída",
  cancelled:         "cancelada",
  // Wave 4 of deep-refactor (2026-05-25): dedicated terminal outcomes for the
  // archival pass over 136 historical specs.
  superseded:        "substituída",
  absorbed:          "absorvida",
  blocked:           "bloqueada",
  "wave-failed":     "wave falhou",
  // Legacy strings from the pre-Wave-4 SQL fallback — kept so an old DB row
  // does not crash the render. New code emits the kebab-case forms above.
  active:            "ativa",
  closed:            "concluída",
  // AC-level statuses (Wave 4, spec `2026-05-21-dashboard-spec-tabs`) — used
  // by `SpecQualityTab` rows. Lifecycle and AC namespaces don't overlap.
  pass:              "passou",
  fail:              "falhou",
  skip:              "pulado",
  unknown:           "pendente",
};

export const STATUS_CLASSES: Record<string, string> = {
  // Tactical-fix `2026-05-21-tf-speccard-polish`: lifecycle statuses get a
  // per-stage hue (was uniform mustard/muted). Each status now reads as its
  // own color so a glance at the list tells stage-of-life immediately. AC
  // statuses (pass/fail/skip/unknown) keep their tonal palette below.
  "no-events":       "bg-muted/40 text-muted-foreground/60",
  planning:          "bg-violet-500/15 text-violet-400",
  implementing:      "bg-green-500/15 text-green-400",
  in_progress:       "bg-green-500/15 text-green-400",
  reviewing:         "bg-amber-500/15 text-amber-400",
  qa:                "bg-emerald-500/15 text-emerald-400",
  // Waves done, QA/close pending — a distinct sky hue reads as "near the finish
  // line" without colliding with qa (emerald) or completed (slate).
  "awaiting-close":  "bg-sky-500/15 text-sky-400",
  "closed-followup": "bg-cyan-500/15 text-cyan-400",
  completed:         "bg-slate-500/15 text-slate-400",
  cancelled:         "bg-[--intent-error]/15/10 text-[--intent-error]/70",
  // Wave 4 of deep-refactor (2026-05-25): dedicated terminal palettes.
  // `superseded` — subdued orange (work was redirected, not dropped).
  // `absorbed`   — light grey (work survives inside a consolidating spec).
  superseded:        "bg-orange-500/15 text-orange-400",
  absorbed:          "bg-slate-400/15 text-slate-300",
  blocked:           "bg-[--intent-error]/15/15 text-[--intent-error]",
  "wave-failed":     "bg-[--intent-error]/15/15 text-[--intent-error]",
  abandoned:         "bg-muted/40 text-muted-foreground/60",
  // Pre-Wave-4 fallback strings (older DB rows).
  active:            "bg-green-500/15 text-green-400",
  closed:            "bg-slate-500/15 text-slate-400",
  // AC-level statuses (Wave 4 of spec `2026-05-21-dashboard-spec-tabs`).
  // Tonal palette revisited by Wave 4 of `2026-05-21-dashboard-spec-tabs-polish`:
  // skip/unknown are split into two distinct grey tints so the eye can still
  // tell "ran-but-skipped" from "never-ran-yet" at a glance.
  pass:              "bg-[--intent-success]/15 text-[--intent-success]",
  fail:              "bg-[--intent-error]/15 text-[--intent-error]",
  skip:              "bg-muted/40 text-muted-foreground",
  unknown:           "bg-muted text-muted-foreground/70",
};

export function StatusPill({ status }: { status: string }) {
  const cls = STATUS_CLASSES[status] ?? "bg-muted text-muted-foreground";
  const label = STATUS_LABELS[status] ?? status;
  return (
    <span
      className={cn(
        "text-[10px] font-medium px-1.5 py-0.5 rounded tracking-wide tabular-nums",
        // Render the empty state label in lowercase (the em-dash already
        // signals "no data" — UPPERCASE would shout it).
        status === "no-events" ? "" : "uppercase",
        cls,
      )}
      title={status}
    >
      {label}
    </span>
  );
}
