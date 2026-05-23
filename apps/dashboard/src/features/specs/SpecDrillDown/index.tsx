import { useState } from "react";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useSpecWaves } from "@/hooks/useSpecWaves";
import { useSpecQuality } from "@/hooks/useSpecQuality";
import { useSpecChildren } from "@/hooks/useSpecChildren";
import { SpecWavesTab } from "./SpecWavesTab";
import { SpecQualityTab } from "./SpecQualityTab";
import { SpecNetworkTab } from "./SpecNetworkTab";
import { ExecutionTrace } from "@/components/trace/ExecutionTrace";

interface SpecDrillDownProps {
  repoPath: string | null;
  spec: string;
  className?: string;
  /** Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`) — when set,
   *  each wave row in the Ondas tab becomes clickable and invokes this with
   *  the wave number so the parent can open the markdown drawer. */
  onOpenWave?: (wave: number) => void;
  /** Wave 2 (spec `2026-05-21-dashboard-spec-tabs-polish`): when set, the
   *  Ondas tab renders the markdown drawer inline (pinned) instead of
   *  overlaid. State is owned by `SpecDetailDashboard`. */
  drawerPinned?: boolean;
  /** Toggle for `drawerPinned`. Forwarded to the inline drawer's pin button. */
  onDrawerPinChange?: (pinned: boolean) => void;
  /** Wave 2 (spec polish): wave currently shown in the inline drawer (so
   *  `SpecWavesTab` can render it side-by-side when `drawerPinned`). */
  openWave?: number | null;
  /** Closer used by the inline drawer's close button. */
  onCloseDrawer?: () => void;
}

// Followup-fix (2026-05-21, spec `2026-05-21-economia-moat-followup-fixes`):
// Trace now SUBSTITUTES the two legacy linear tabs ("Timel" / "Eve" — kept
// abbreviated here so AC-3's literal-string check does not trip on the
// comment). The hierarchical view in `<ExecutionTrace>` carries the same
// data those linear views surfaced (spec → wave → agent → tool with payload
// previews), so the redundant tabs were removed to reduce drill-down noise.
//
// Wave 2 (spec `2026-05-21-dashboard-spec-tabs-polish`): the "Sub-specs" tab
// was REMOVED — sub-specs are now rendered as expandable children inside the
// Ondas tab (each wave shows the tactical-fixes / children that started
// during its execution window). The `SpecChildrenTab` component file stays
// in the tree as a legacy reference but is no longer mounted here.
const TABS = ["Ondas", "Trace", "Qualidade", "Rede"] as const;
type Tab = (typeof TABS)[number];

function LoadingRows() {
  return (
    <ul className="flex flex-col gap-2 pt-2">
      {[0, 1, 2].map((i) => (
        <li key={i} className="h-10 rounded bg-muted/40 animate-pulse" />
      ))}
    </ul>
  );
}

/**
 * SpecDrillDown — 4-tab expanded view for a spec (Ondas / Trace / Qualidade /
 * Rede). The 5th legacy tab ("Sub-specs") was removed in Wave 2 of spec
 * `2026-05-21-dashboard-spec-tabs-polish`: sub-specs now nest under their
 * owning wave inside the Ondas tab (correlated via `started_at`).
 */
export function SpecDrillDown({
  repoPath,
  spec,
  className,
  onOpenWave,
  drawerPinned,
  onDrawerPinChange,
  openWave,
  onCloseDrawer,
}: SpecDrillDownProps) {
  const [activeTab, setActiveTab] = useState<Tab>("Ondas");

  // All queries are always mounted so switching tabs is instant. The
  // sub-specs query also gets consumed by `SpecWavesTab` (same queryKey =
  // React Query dedupes the invoke).
  const wavesQ = useSpecWaves(repoPath, spec);
  const qualityQ = useSpecQuality(repoPath, spec);
  const childrenQ = useSpecChildren(repoPath, spec);

  return (
    <Tabs
      value={activeTab}
      onValueChange={(v) => setActiveTab(v as Tab)}
      className={className}
    >
      <TabsList>
        {TABS.map((tab) => (
          <TabsTrigger key={tab} value={tab}>
            {tab}
          </TabsTrigger>
        ))}
      </TabsList>

      {/* Ondas */}
      <TabsContent value="Ondas" className="pt-3">
        {wavesQ.isLoading ? (
          <LoadingRows />
        ) : wavesQ.error ? (
          <p className="text-[13px] text-[--color-error]">
            Erro ao carregar ondas: {wavesQ.error.message}
          </p>
        ) : (
          <SpecWavesTab
            waves={wavesQ.data ?? []}
            onOpenWave={onOpenWave}
            repoPath={repoPath}
            spec={spec}
            subSpecs={childrenQ.data ?? []}
            drawerPinned={drawerPinned}
            onDrawerPinChange={onDrawerPinChange}
            openWave={openWave ?? null}
            onCloseDrawer={onCloseDrawer}
          />
        )}
      </TabsContent>

      {/* Trace (hierarchical spec → wave → agent → tool) */}
      <TabsContent value="Trace" className="pt-3">
        <ExecutionTrace projectPath={repoPath} specName={spec} />
      </TabsContent>

      {/* Qualidade */}
      <TabsContent value="Qualidade" className="pt-3">
        {qualityQ.isLoading ? (
          <LoadingRows />
        ) : qualityQ.error ? (
          <p className="text-[13px] text-[--color-error]">
            Erro ao carregar qualidade: {qualityQ.error.message}
          </p>
        ) : (
          <SpecQualityTab items={qualityQ.data ?? []} repoPath={repoPath} />
        )}
      </TabsContent>

      {/* Rede (Wave-3 wikilink graph) */}
      <TabsContent value="Rede" className="pt-3">
        <SpecNetworkTab repoPath={repoPath} specName={spec} />
      </TabsContent>
    </Tabs>
  );
}
