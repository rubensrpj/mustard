import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { dashboardSpecCard } from "@/lib/dashboard";
import type { SpecCard as SpecCardData } from "@/lib/types/specs";
import { SpecCard } from "../SpecCard";
import { SpecDrillDown } from "../SpecDrillDown";
import { WaveMarkdownDrawer } from "../WaveMarkdownDrawer";
import { useSpecWaves } from "@/hooks/useSpecWaves";

interface SpecDetailDashboardProps {
  repoPath: string | null;
  spec: string;
  className?: string;
}

/**
 * Per-tab container for an opened spec. The header now reuses `<SpecCard>`
 * verbatim (tactical-fix `2026-05-21-tf-detail-uses-speccard`) so the
 * detail view stays bit-for-bit identical to the list. `onOpenSpec` is
 * intentionally omitted — the "Detalhes" button hides itself since we are
 * already inside the detail. Body keeps `<SpecDrillDown>` (Ondas / Trace /
 * Qualidade / Rede / Sub-specs).
 */
export function SpecDetailDashboard({
  repoPath,
  spec,
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

  // Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`) — track the
  // wave currently shown in the markdown drawer. `null` = drawer closed.
  // We also pull the waves list here (deduped by React Query against the same
  // key used inside `SpecDrillDown`) so the drawer can show the role label.
  const [openWave, setOpenWave] = useState<number | null>(null);
  // Wave 2 (spec `2026-05-21-dashboard-spec-tabs-polish`): pin toggle. When
  // pinned, the drawer renders inline beside the waves list (no overlay).
  // State lives here (not in `WaveMarkdownDrawer`) so the choice survives
  // close/reopen cycles and so `SpecWavesTab` can switch its layout to a
  // side-by-side flex when pinned.
  const [drawerPinned, setDrawerPinned] = useState<boolean>(false);
  const wavesQ = useSpecWaves(repoPath, spec);
  const openWaveRole =
    openWave != null
      ? openWave === 0
        ? "spec principal"
        : (wavesQ.data?.find((w) => w.wave === openWave)?.role ?? null)
      : null;

  return (
    <div className={className}>
      {/* Header: same `<SpecCard>` rendered by the list — single source of
          visual truth. `onOpenSpec` is omitted so the "Detalhes" button hides
          itself (we're already inside the detail view). */}
      <div className="mb-4">
        {cardQ.data ? (
          <SpecCard data={cardQ.data} repoPath={repoPath} />
        ) : (
          <div className="h-32 bg-muted/40 rounded-lg animate-pulse" />
        )}
      </div>

      {/* Body: 4 sub-tabs. When the drawer is pinned the markdown panel is
          rendered inline inside `SpecWavesTab` (next to the waves list), so
          we only mount the overlay Sheet down here when `!drawerPinned`. */}
      <SpecDrillDown
        repoPath={repoPath}
        spec={spec}
        onOpenWave={setOpenWave}
        drawerPinned={drawerPinned}
        onDrawerPinChange={setDrawerPinned}
        openWave={openWave}
        onCloseDrawer={() => setOpenWave(null)}
      />

      {/* Wave 2: overlay markdown drawer for the clicked wave row. When
          `drawerPinned` is true the inline `<aside>` inside `SpecWavesTab`
          is the rendering site instead — this overlay stays mounted but
          `open={false}` so the Sheet stays collapsed. */}
      {!drawerPinned && (
        <WaveMarkdownDrawer
          open={openWave != null}
          onOpenChange={(o) => !o && setOpenWave(null)}
          repoPath={repoPath}
          spec={spec}
          wave={openWave}
          role={openWaveRole}
          pinned={false}
          onPinChange={setDrawerPinned}
        />
      )}
    </div>
  );
}
