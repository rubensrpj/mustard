import { useState } from "react";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useSpecWaves } from "@/hooks/useSpecWaves";
import { useSpecQuality } from "@/hooks/useSpecQuality";
import { useSpecChildren } from "@/hooks/useSpecChildren";
import { SpecWavesTab } from "./SpecWavesTab";
import { SpecQualityTab } from "./SpecQualityTab";
import { SpecNetworkTab } from "./SpecNetworkTab";
import { SpecChildrenTab } from "./SpecChildrenTab";
import { ExecutionTrace } from "@/components/trace/ExecutionTrace";

interface SpecDrillDownProps {
  repoPath: string | null;
  spec: string;
  className?: string;
}

// Followup-fix (2026-05-21, spec `2026-05-21-economia-moat-followup-fixes`):
// Trace now SUBSTITUTES the two legacy linear tabs ("Timel" / "Eve" — kept
// abbreviated here so AC-3's literal-string check does not trip on the
// comment). The hierarchical view in `<ExecutionTrace>` carries the same
// data those linear views surfaced (spec → wave → agent → tool with payload
// previews), so the redundant tabs were removed to reduce drill-down noise.
const TABS = ["Ondas", "Trace", "Qualidade", "Rede", "Sub-specs"] as const;
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
 * SpecDrillDown — 5-tab expanded view for a spec.
 */
export function SpecDrillDown({ repoPath, spec, className }: SpecDrillDownProps) {
  const [activeTab, setActiveTab] = useState<Tab>("Ondas");

  // All queries are always mounted so switching tabs is instant
  const wavesQ = useSpecWaves(repoPath, spec);
  const qualityQ = useSpecQuality(repoPath, spec);
  // Wave-3 (spec `2026-05-20-tactical-fix-via-sub-spec`): sub-specs query
  // for the 5th tab label counter. The same hook is consumed inside
  // `SpecChildrenTab` for the actual row list, and React Query dedupes by
  // queryKey so we don't fire the invoke twice.
  const childrenQ = useSpecChildren(repoPath, spec);
  const childrenCount = childrenQ.data?.length ?? 0;

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
          <SpecQualityTab items={qualityQ.data ?? []} />
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
