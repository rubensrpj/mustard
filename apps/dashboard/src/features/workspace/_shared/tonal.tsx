import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * A tonal color value — a CSS color the icon and its fill derive from. Pass a
 * design-system variable reference (`var(--intent-info)` …) or a raw brand hex
 * (`#512bd4`). The fill is computed at render via `color-mix`, NOT a Tailwind
 * opacity modifier: a Tailwind opacity modifier over a hex CSS var produces
 * `rgb(#hex / 0.1)` (invalid CSS), so the tonal square silently
 * dropped out and the icon read as flat grey. `color-mix` is supported in the
 * Tauri WebView (recent Chromium), so we mix the color with `transparent` for
 * the fill and let the icon inherit `currentColor`.
 */
export type TonalColor = string;

/** Design-system intent tokens, as `color-mix`-safe CSS color references. */
export const TONE = {
  success: "var(--intent-success)",
  error: "var(--intent-error)",
  warning: "var(--intent-warning)",
  info: "var(--intent-info)",
  accent: "var(--accent)",
  primary: "var(--primary)",
  muted: "var(--muted-foreground)",
} as const;

/** Build the inline `style` for a tonal surface — `color` drives the glyph
 *  (via `currentColor`) and a 14%-mixed fill behaves as the old `/10` square. */
export function tonalStyle(color: TonalColor): React.CSSProperties {
  return {
    color,
    backgroundColor: `color-mix(in srgb, ${color} 14%, transparent)`,
  };
}

/**
 * Shared tonal icon container — `h-7 w-7 rounded-md` with the intent fill +
 * matching glyph, the one treatment every overview card uses. Color is applied
 * via inline style (see {@link tonalStyle}), never a Tailwind opacity class.
 */
export function TonalIcon({
  icon: Icon,
  color,
  className,
  pulse,
}: {
  icon: LucideIcon;
  color: TonalColor;
  className?: string;
  pulse?: boolean;
}) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md",
        className,
      )}
      style={tonalStyle(color)}
    >
      <Icon className={cn("h-3.5 w-3.5", pulse && "animate-pulse")} />
    </span>
  );
}

// --- Conventional-commit typing (GitInfoCard history) ---------------------

/** Conventional-commit type → tonal color. `feat`/`perf` read as success,
 *  `fix` warning, `refactor`/`docs`/`test` info, `style` accent, `revert`
 *  error, the build chores muted, and anything unrecognised muted. */
const COMMIT_TYPE_COLOR: Record<string, TonalColor> = {
  feat: TONE.success,
  perf: TONE.success,
  fix: TONE.warning,
  refactor: TONE.info,
  docs: TONE.info,
  test: TONE.info,
  style: TONE.accent,
  chore: TONE.muted,
  build: TONE.muted,
  ci: TONE.muted,
  revert: TONE.error,
};

/** Parse the conventional-commit type from a subject (`feat(scope)!: …`) and
 *  return its short type + tonal color. Falls back to a muted "·" when the
 *  subject carries no recognised type prefix. */
export function commitType(subject: string): { type: string; color: TonalColor } {
  const m = subject.match(/^(\w+)(\(.+?\))?!?:/);
  const raw = m?.[1]?.toLowerCase();
  if (raw && raw in COMMIT_TYPE_COLOR) {
    return { type: raw, color: COMMIT_TYPE_COLOR[raw] };
  }
  return { type: "", color: TONE.muted };
}
