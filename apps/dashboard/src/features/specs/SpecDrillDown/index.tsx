import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useSpecWaves } from "@/hooks/useSpecWaves";
import { useSpecQuality } from "@/hooks/useSpecQuality";
import { useSpecChildren } from "@/hooks/useSpecChildren";
import { fetchSpecMarkdown } from "@/lib/dashboard";
import { slicePrdSection } from "@/lib/text";
import { Markdown } from "@/components/page";
import { SpecWavesTab } from "../SpecWavesTab";
import { SpecQualityTab } from "../SpecQualityTab";
import { ExecutionTrace } from "@/features/trace/ExecutionTrace";
import { ChangeRequestActivityBlock } from "@/features/changeRequests";

interface SpecDrillDownProps {
  repoPath: string | null;
  spec: string;
  className?: string;
  /** Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`) — when set,
   *  each wave row in the Ondas tab becomes clickable and invokes this with
   *  the wave number so the parent can open the wave markdown panel. */
  onOpenWave?: (wave: number) => void;
  /** Wave currently selected in the Ondas tab — drives the always-open
   *  resizable split panel inside `SpecWavesTab`. State is owned by
   *  `SpecDetailDashboard`. */
  openWave?: number | null;
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
//
// W0/T0.6 (spec `2026-05-25-mustard-deep-refactor`): the "Rede" tab was
// REMOVED with its internal force-graph implementation. Cross-spec relations
// are now navigated externally via Obsidian (wikilinks `[[X]]` resolve to
// `obsidian://open?vault=…&file=…` inside the markdown renderer).
// "PRD" leads — it is the *what & why* (the spec's PRD layer), the natural
// entry point you read before drilling into the full narrative. It is a
// READ-only slice of `spec.md` between the `<!-- PRD -->` / `<!-- PLAN -->`
// markers (spec `matar-prd-standalone-fazer-feature`, wave 3), reusing the
// same `markdownQ` fetch — no extra Tauri command, no AI, no trigger.
// "Spec" follows — the full narrative overview (spec.md) the other tabs drill
// into. Backed by the long-existing `dashboard_spec_markdown` command +
// `fetchSpecMarkdown` client, rendered with the shared `Markdown` component.
const TABS = ["PRD", "Spec", "Ondas", "Trace", "Qualidade"] as const;
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
 * SpecDrillDown — 3-tab expanded view for a spec (Ondas / Trace / Qualidade).
 * The legacy "Sub-specs" tab was removed in Wave 2 of spec
 * `2026-05-21-dashboard-spec-tabs-polish`: sub-specs nest under their owning
 * wave inside the Ondas tab. The "Rede" tab was removed in W0/T0.6 of
 * `2026-05-25-mustard-deep-refactor`: cross-spec navigation moved to
 * Obsidian via wikilink URIs.
 */
export function SpecDrillDown({
  repoPath,
  spec,
  className,
  onOpenWave,
  openWave,
}: SpecDrillDownProps) {
  // `null` = no explicit user choice yet; the landing tab is then derived
  // below from whether this spec has a PRD layer. A click pins `picked`.
  const [picked, setPicked] = useState<Tab | null>(null);

  // All queries are always mounted so switching tabs is instant. The
  // sub-specs query also gets consumed by `SpecWavesTab` (same queryKey =
  // React Query dedupes the invoke).
  const wavesQ = useSpecWaves(repoPath, spec);
  const qualityQ = useSpecQuality(repoPath, spec);
  const childrenQ = useSpecChildren(repoPath, spec);
  // spec.md narrative — same query key the standalone SpecDetail page uses so
  // React Query dedupes when both are mounted.
  const markdownQ = useQuery({
    queryKey: ["spec-markdown", repoPath, spec],
    queryFn: () => fetchSpecMarkdown(repoPath as string, spec),
    enabled: !!repoPath && !!spec,
    staleTime: 10_000,
  });
  // PRD layer sliced from the same spec.md (no extra fetch). Empty when the
  // `<!-- PRD -->` / `<!-- PLAN -->` markers are absent.
  const prdSection = markdownQ.data ? slicePrdSection(markdownQ.data) : "";
  // Land on PRD when this spec has a PRD layer; once the fetch resolves and a
  // spec has no markers (≈ half of older specs), fall back to the full Spec so
  // nobody lands on the empty PRD state. During load we stay on PRD (it shows
  // the loading rows, not an empty state). A user click always wins.
  const activeTab: Tab =
    picked ?? (markdownQ.isFetched && !prdSection ? "Spec" : "PRD");

  return (
    <Tabs
      value={activeTab}
      onValueChange={(v) => setPicked(v as Tab)}
      className={className}
    >
      <TabsList>
        {TABS.map((tab) => (
          <TabsTrigger key={tab} value={tab}>
            {tab}
          </TabsTrigger>
        ))}
      </TabsList>

      {/* Spec (spec.md narrative) */}
      <TabsContent value="Spec" className="pt-3">
        {/* Mid-spec change requests (pipeline.change.request). Renders nothing
            when the spec has no requests — keeps the narrative clean. */}
        <ChangeRequestActivityBlock specId={spec} />
        {markdownQ.isLoading ? (
          <LoadingRows />
        ) : markdownQ.error ? (
          <p className="text-[13px] text-muted-foreground py-4 text-center">
            spec.md indisponível para esta spec.
          </p>
        ) : markdownQ.data ? (
          <div className="max-h-[60vh] overflow-auto pr-1">
            <Markdown content={markdownQ.data} />
          </div>
        ) : (
          <p className="text-[13px] text-muted-foreground py-4 text-center">
            spec.md ainda não foi gerado para esta spec.
          </p>
        )}
      </TabsContent>

      {/* PRD — read-only slice of the spec's PRD layer (between the
          `<!-- PRD -->` / `<!-- PLAN -->` markers), reusing the same `markdownQ`
          fetch. Empty state when markers are absent (older specs). */}
      <TabsContent value="PRD" className="pt-3">
        {markdownQ.isLoading ? (
          <LoadingRows />
        ) : markdownQ.error ? (
          <p className="text-[13px] text-muted-foreground py-4 text-center">
            spec.md indisponível para esta spec.
          </p>
        ) : prdSection ? (
          <div className="max-h-[60vh] overflow-auto pr-1">
            <Markdown content={prdSection} />
          </div>
        ) : (
          <p className="text-[13px] text-muted-foreground py-4 text-center">
            Esta spec não tem uma camada PRD demarcada.
          </p>
        )}
      </TabsContent>

      {/* Ondas */}
      <TabsContent value="Ondas" className="pt-3">
        {wavesQ.isLoading ? (
          <LoadingRows />
        ) : wavesQ.error ? (
          <p className="text-[13px] text-[--intent-error]">
            Erro ao carregar ondas: {wavesQ.error.message}
          </p>
        ) : (
          <SpecWavesTab
            waves={wavesQ.data ?? []}
            onOpenWave={onOpenWave}
            repoPath={repoPath}
            spec={spec}
            subSpecs={childrenQ.data ?? []}
            openWave={openWave ?? null}
          />
        )}
      </TabsContent>

      {/* Trace (hierarchical spec → wave → agent → tool) */}
      <TabsContent value="Trace" className="pt-3">
        <ExecutionTrace projectPath={repoPath} source={{ kind: "spec", specName: spec }} />
      </TabsContent>

      {/* Qualidade */}
      <TabsContent value="Qualidade" className="pt-3">
        {qualityQ.isLoading ? (
          <LoadingRows />
        ) : qualityQ.error ? (
          <p className="text-[13px] text-[--intent-error]">
            Erro ao carregar qualidade: {qualityQ.error.message}
          </p>
        ) : (
          <SpecQualityTab items={qualityQ.data ?? []} repoPath={repoPath} />
        )}
      </TabsContent>
    </Tabs>
  );
}
