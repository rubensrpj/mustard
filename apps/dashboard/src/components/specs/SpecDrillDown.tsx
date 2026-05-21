import { useMemo, useState } from "react";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useSpecWaves } from "@/hooks/useSpecWaves";
import { useSpecQuality } from "@/hooks/useSpecQuality";
import { useSpecTimeline } from "@/hooks/useSpecTimeline";
import { useSpecEvents } from "@/hooks/useSpecEvents";
import { useSpecChildren } from "@/hooks/useSpecChildren";
import { SpecWavesTab } from "./SpecWavesTab";
import { SpecQualityTab } from "./SpecQualityTab";
import { SpecTimelineTab } from "./SpecTimelineTab";
import { SpecEventsTab } from "./SpecEventsTab";
import { SpecNetworkTab } from "./SpecNetworkTab";
import { SpecChildrenTab } from "./SpecChildrenTab";
import { ExecutionTrace } from "@/components/trace/ExecutionTrace";
import type { SpecTimelineNode, EventFilter } from "@/lib/types/specs";

interface SpecDrillDownProps {
  repoPath: string | null;
  spec: string;
  className?: string;
}

// Wave 6 (2026-05-21): "Trace" added as the hierarchical view of the same
// data the linear "Eventos" tab surfaces. Eventos stays as the linear
// fallback for users who want a flat timeline; Trace is the recommended
// default for diagnosis. Both coexist — see Wave 6 spec §Tarefas.
const TABS = ["Ondas", "Trace", "Qualidade", "Timeline", "Eventos", "Rede", "Sub-specs"] as const;
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
 * SpecDrillDown — 4-tab expanded view for a spec.
 *
 * AC-7: this file MUST contain the literal strings Ondas, Qualidade, Timeline,
 * Eventos as tab labels. They appear in the TABS constant and JSX below.
 */
export function SpecDrillDown({ repoPath, spec, className }: SpecDrillDownProps) {
  const [activeTab, setActiveTab] = useState<Tab>("Ondas");
  const [eventsFilter, setEventsFilter] = useState<EventFilter | undefined>();

  // All queries are always mounted so switching tabs is instant
  const wavesQ = useSpecWaves(repoPath, spec);
  const qualityQ = useSpecQuality(repoPath, spec);
  const timelineQ = useSpecTimeline(repoPath, spec);
  const eventsQ = useSpecEvents(repoPath, spec, eventsFilter);
  // Wave-3 (spec `2026-05-20-tactical-fix-via-sub-spec`): sub-specs query
  // for the 5th tab label counter. The same hook is consumed inside
  // `SpecChildrenTab` for the actual row list, and React Query dedupes by
  // queryKey so we don't fire the invoke twice.
  const childrenQ = useSpecChildren(repoPath, spec);
  const childrenCount = childrenQ.data?.length ?? 0;

  // Wave numbers known for this spec — fed to nested tabs that surface
  // per-wave deep links (Timeline). Empty list when waves query is loading.
  const waveNumbers = useMemo<number[]>(() => {
    return wavesQ.data?.map((w) => w.wave) ?? [];
  }, [wavesQ.data]);

  function handleTimelineNodeClick(node: SpecTimelineNode) {
    // Navigate to Eventos tab and pre-filter by wave if present
    setEventsFilter(node.wave != null ? { wave: node.wave } : undefined);
    setActiveTab("Eventos");
  }

  return (
    <Tabs
      value={activeTab}
      onValueChange={(v) => setActiveTab(v as Tab)}
      className={className}
    >
      <TabsList>
        {TABS.map((tab) => (
          <TabsTrigger key={tab} value={tab}>
            {tab === "Sub-specs" && childrenCount > 0 ? (
              <>
                Sub-specs{" "}
                <span
                  className="ml-1 tabular-nums text-muted-foreground"
                  style={{ fontVariantNumeric: "tabular-nums" }}
                >
                  ({childrenCount})
                </span>
              </>
            ) : (
              tab
            )}
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
          <SpecWavesTab waves={wavesQ.data ?? []} />
        )}
      </TabsContent>

      {/* Trace (Wave 6 — hierarchical spec → wave → agent → tool) */}
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
          <SpecQualityTab items={qualityQ.data ?? []} />
        )}
      </TabsContent>

      {/* Timeline */}
      <TabsContent value="Timeline" className="pt-3">
        {timelineQ.isLoading ? (
          <LoadingRows />
        ) : timelineQ.error ? (
          <p className="text-[13px] text-[--color-error]">
            Erro ao carregar timeline: {timelineQ.error.message}
          </p>
        ) : (
          <SpecTimelineTab
            nodes={timelineQ.data ?? []}
            onNodeClick={handleTimelineNodeClick}
            repoPath={repoPath}
            spec={spec}
            waves={waveNumbers}
          />
        )}
      </TabsContent>

      {/* Eventos */}
      <TabsContent value="Eventos" className="pt-3">
        {eventsQ.isLoading ? (
          <LoadingRows />
        ) : eventsQ.error ? (
          <p className="text-[13px] text-[--color-error]">
            Erro ao carregar eventos: {eventsQ.error.message}
          </p>
        ) : (
          <SpecEventsTab
            events={eventsQ.data ?? []}
            initialFilter={eventsFilter}
          />
        )}
      </TabsContent>

      {/* Rede (Wave-3 wikilink graph) */}
      <TabsContent value="Rede" className="pt-3">
        <SpecNetworkTab repoPath={repoPath} specName={spec} />
      </TabsContent>

      {/* Sub-specs (Wave-3, spec 2026-05-20-tactical-fix-via-sub-spec) */}
      <TabsContent value="Sub-specs" className="pt-3">
        <SpecChildrenTab repoPath={repoPath} parent={spec} />
      </TabsContent>
    </Tabs>
  );
}
