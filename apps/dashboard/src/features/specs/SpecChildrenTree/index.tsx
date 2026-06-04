import { useQuery } from "@tanstack/react-query";
import { fetchSpecChildrenTree } from "@/lib/dashboard";
import { useSpecWaves } from "@/hooks/useSpecWaves";
import { useSpecQuality } from "@/hooks/useSpecQuality";
import { SpecChildRow } from "../SpecChildRow";
import { useT } from "@/lib/i18n";

interface SpecChildrenTreeProps {
  spec: string;
  projectPath: string;
  /** Open the parent drill-down — children rows defer to this. */
  onOpenParent: (slug: string) => void;
}

/**
 * SpecChildrenTree — the lazily-loaded body that unfurls under an expanded
 * `SpecRow`. Renders the same enriched per-spec data the detail drill-down
 * uses, compactly:
 *
 *   - waves   → `dashboard_spec_waves` (`spec_waves_v2`): carries `role`
 *               + `summary` parsed from `wave-plan.md`. The bare
 *               `spec_children_tree` projection leaves `role` empty and has
 *               no summary, which is why this inline tree used to read
 *               "ONDA 1" with no detail.
 *   - ACs     → `dashboard_spec_quality` (`spec_quality_v2`): carries the real
 *               `ac_label` description (the spec.md text, not the bare id —
 *               which is why ACs used to render "AC-1 AC-1") plus the
 *               pass/fail status.
 *   - subspecs → `spec_children_tree`: the cross-developer UNION (events +
 *               `### Parent:` headers) is only available here, so we keep this
 *               query for the sub-spec rows.
 *
 * React Query dedupes the waves/quality queries against the detail view (same
 * `["spec-waves", …]` / `["spec-quality", …]` keys), so expanding a row that
 * was already drilled into never refetches.
 */
export function SpecChildrenTree({
  spec,
  projectPath,
  onOpenParent,
}: SpecChildrenTreeProps) {
  const t = useT();
  const wavesQ = useSpecWaves(projectPath, spec);
  const qualityQ = useSpecQuality(projectPath, spec);
  const childrenQ = useQuery({
    queryKey: ["spec-children-tree", spec, projectPath],
    queryFn: () => fetchSpecChildrenTree(spec, projectPath),
    staleTime: 30_000,
  });

  const isLoading =
    wavesQ.isLoading || qualityQ.isLoading || childrenQ.isLoading;
  const isError = wavesQ.isError && qualityQ.isError && childrenQ.isError;

  if (isLoading) {
    return (
      <div className="flex flex-col gap-1 pb-1">
        {[0, 1].map((i) => (
          <div key={i} className="h-8 ml-12 mr-4 rounded-md bg-muted/20 animate-pulse" />
        ))}
      </div>
    );
  }

  if (isError) {
    return (
      <p className="pl-12 pr-4 py-1.5 text-[11px] text-muted-foreground/70">
        {t("route.specs.children_error", "Não foi possível carregar os filhos.")}
      </p>
    );
  }

  const waves = wavesQ.data ?? [];
  const acs = qualityQ.data ?? [];
  const subspecs = childrenQ.data?.subspecs ?? [];

  const empty = waves.length === 0 && acs.length === 0 && subspecs.length === 0;

  if (empty) {
    return (
      <p className="pl-12 pr-4 py-1.5 text-[11px] text-muted-foreground/60">
        {t("route.specs.children_empty", "Sem ondas, ACs ou sub-specs.")}
      </p>
    );
  }

  return (
    <div className="flex flex-col pb-1">
      {waves.map((w) => (
        <SpecChildRow
          key={`w-${w.wave}`}
          kind="wave"
          label={w.role ? `${w.wave} · ${w.role}` : String(w.wave)}
          detail={w.summary}
          status={w.status}
          onClick={() => onOpenParent(spec)}
        />
      ))}
      {acs.map((ac) => (
        <SpecChildRow
          key={`ac-${ac.ac_id}`}
          kind="ac"
          label={ac.ac_id}
          detail={ac.ac_label}
          status={ac.status}
          onClick={() => onOpenParent(spec)}
        />
      ))}
      {subspecs.map((s) => (
        <SpecChildRow
          key={`s-${s.spec}`}
          kind="sub-spec"
          label={s.spec}
          detail={s.reason}
          state={s.state}
          onClick={() => onOpenParent(spec)}
        />
      ))}
    </div>
  );
}
