import { shortSpecName } from "@/lib/phaseTheme";
import { cn } from "@/lib/utils";

/**
 * Label used in a wave-child row to identify which parent spec it belongs to.
 * Renders: `W{N} · {role} · {parentName}` with parent name in muted mono so
 * scroll context survives — the user always knows the parent even when the
 * parent row scrolled off-screen.
 */
export interface WaveRowLabelProps {
  /** Wave number (parsed from name like `wave-3-fullstack` → 3). */
  waveNumber: number | null;
  /** Role string (backend, frontend, fullstack, cleanup, etc). */
  role: string;
  /** Parent spec full name — shown as muted lineage text. */
  parentName: string;
  className?: string;
}

export function WaveRowLabel({
  waveNumber,
  role,
  parentName,
  className,
}: WaveRowLabelProps) {
  return (
    <div className={cn("flex items-center gap-2 min-w-0", className)}>
      {waveNumber !== null && !isNaN(waveNumber) && (
        <span
          className="inline-flex items-center rounded-md px-1.5 py-0 text-[10px] font-medium bg-primary/15 text-primary border border-primary/30 tabular-nums shrink-0"
          title={`Wave ${waveNumber} desta spec mãe`}
        >
          W{waveNumber}
        </span>
      )}
      <span
        className="text-[12px] text-foreground/80 font-medium shrink-0"
        title={`Papel desta wave: ${role}`}
      >
        {role}
      </span>
      <span className="text-muted-foreground/40 shrink-0">·</span>
      <span
        className="text-[11px] text-muted-foreground/70 font-mono truncate"
        title={`Wave de: ${parentName}`}
      >
        {shortSpecName(parentName)}
      </span>
    </div>
  );
}
