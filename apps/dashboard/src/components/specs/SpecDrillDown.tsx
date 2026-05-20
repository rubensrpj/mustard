import { useState } from "react";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useSpecWaves } from "@/hooks/useSpecWaves";
import { useSpecQuality } from "@/hooks/useSpecQuality";
import { useSpecTimeline } from "@/hooks/useSpecTimeline";
import { useSpecEvents } from "@/hooks/useSpecEvents";
import { SpecWavesTab } from "./SpecWavesTab";
import { SpecQualityTab } from "./SpecQualityTab";
import { SpecTimelineTab } from "./SpecTimelineTab";
import { SpecEventsTab } from "./SpecEventsTab";
import type { SpecTimelineNode, EventFilter } from "@/lib/types/specs";

interface SpecDrillDownProps {
  repoPath: string | null;
  spec: string;
  className?: string;
}

const TABS = ["Ondas", "Qualidade", "Timeline", "Eventos"] as const;
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

  // All 4 queries are always mounted so switching tabs is instant
  const wavesQ = useSpecWaves(repoPath, spec);
  const qualityQ = useSpecQuality(repoPath, spec);
  const timelineQ = useSpecTimeline(repoPath, spec);
  const eventsQ = useSpecEvents(repoPath, spec, eventsFilter);

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
          <SpecWavesTab waves={wavesQ.data ?? []} />
        )}
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
    </Tabs>
  );
}
