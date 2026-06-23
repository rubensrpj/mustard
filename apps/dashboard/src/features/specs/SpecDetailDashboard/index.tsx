import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { dashboardSpecCard } from "@/lib/dashboard";
import type { SpecCard as SpecCardData } from "@/lib/types/specs";
import { SpecCard } from "../SpecCard";
import { SpecDrillDown } from "../SpecDrillDown";
import { useSpecWaves } from "@/hooks/useSpecWaves";

interface SpecDetailDashboardProps {
  repoPath: string | null;
  spec: string;
  /** Wave to pre-select in the Ondas split when the tab mounts (or when the
   *  list re-opens this spec on a different wave). Set when the spec was opened
   *  by clicking a wave-child in the list tree; omitted for a plain open. */
  initialWave?: number;
  /** Bumped by the list on every wave-bearing open. The re-select effect keys
   *  on this (not on `initialWave`) so re-clicking the SAME wave-child after the
   *  user closed the panel re-opens it instead of no-opping. */
  initialWaveNonce?: number;
  className?: string;
}

/**
 * Per-tab container for an opened spec. The header now reuses `<SpecCard>`
 * verbatim (tactical-fix `2026-05-21-tf-detail-uses-speccard`) so the
 * detail view stays bit-for-bit identical to the list. `onOpenSpec` is
 * intentionally omitted — the "Detalhes" button hides itself since we are
 * already inside the detail. Body keeps `<SpecDrillDown>` (PRD / Spec / Ondas
 * / Trace / Qualidade).
 */
export function SpecDetailDashboard({
  repoPath,
  spec,
  initialWave,
  initialWaveNonce,
  className,
}: SpecDetailDashboardProps) {
  const cardQ = useQuery({
    queryKey: ["spec-card", repoPath, spec],
    queryFn: (): Promise<SpecCardData> => dashboardSpecCard(repoPath!, spec),
    enabled: !!repoPath,
    staleTime: 5_000,
    refetchInterval: 5_000,
    refetchIntervalInBackground: false,
  });

  // Selected wave shown in the Ondas split panel. `null` = no wave selected
  // (panel hidden). Spec `melhorias-no-dashboard-destacar-projeto` (wave 2):
  // the legacy overlay `<Sheet>` drawer + pin toggle were removed — the wave
  // detail now lives in an always-open resizable split owned by
  // `SpecWavesTab`. This component only tracks which wave is open.
  const [openWave, setOpenWave] = useState<number | null>(
    initialWave ?? null,
  );
  // Re-sync the selected wave when the list (re-)opens this tab on a wave.
  // Keyed on `initialWaveNonce` (bumped on every wave-bearing open) so it fires
  // even on a same-wave re-click after the user closed the panel — keying on
  // `initialWave` alone would no-op then. A plain re-open carries no nonce, so
  // the user's in-tab selection is left untouched.
  useEffect(() => {
    if (initialWave != null) setOpenWave(initialWave);
  }, [initialWave, initialWaveNonce]);
  // Waves list (deduped by React Query against the same key used inside
  // `SpecDrillDown`) so the `<SpecCard>` footer can point at the live wave.
  const wavesQ = useSpecWaves(repoPath, spec);

  return (
    <div className={className}>
      {/* Header: same `<SpecCard>` rendered by the list — single source of
          visual truth. `onOpenSpec` is omitted so the "Detalhes" button hides
          itself (we're already inside the detail view). */}
      <div className="mb-4">
        {cardQ.data ? (
          <SpecCard data={cardQ.data} repoPath={repoPath} waves={wavesQ.data} />
        ) : (
          <div className="h-32 bg-muted/40 rounded-lg animate-pulse" />
        )}
      </div>

      {/* Body: drill-down tabs. The Ondas tab opens the wave markdown panel
          inline (resizable split) in response to `onOpenWave`. */}
      <SpecDrillDown
        repoPath={repoPath}
        spec={spec}
        onOpenWave={setOpenWave}
        openWave={openWave}
      />
    </div>
  );
}
