import { useQuery } from "@tanstack/react-query";
import { fetchSpecChildrenTree } from "@/lib/dashboard";
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
 * `SpecRow`. Owns its own `useQuery` so the fetch only fires once the spec is
 * expanded; React Query caches by `[spec, projectPath]` so collapse + re-expand
 * never refetches within `staleTime`.
 *
 * Mounted only while expanded (the page conditionally renders it), so the
 * `enabled` flag would be redundant — but we key the query the way the wave
 * spec describes so the cache survives the unmount/remount of this component.
 */
export function SpecChildrenTree({
  spec,
  projectPath,
  onOpenParent,
}: SpecChildrenTreeProps) {
  const t = useT();
  const { data, isLoading, isError } = useQuery({
    queryKey: ["spec-children-tree", spec, projectPath],
    queryFn: () => fetchSpecChildrenTree(spec, projectPath),
    staleTime: 30_000,
  });

  if (isLoading) {
    return (
      <div className="flex flex-col gap-1 pb-1">
        {[0, 1].map((i) => (
          <div key={i} className="h-8 ml-12 mr-4 rounded-md bg-muted/20 animate-pulse" />
        ))}
      </div>
    );
  }

  if (isError || !data) {
    return (
      <p className="pl-12 pr-4 py-1.5 text-[11px] text-muted-foreground/70">
        {t("route.specs.children_error", "Não foi possível carregar os filhos.")}
      </p>
    );
  }

  const empty =
    data.waves.length === 0 &&
    data.acs.length === 0 &&
    data.subspecs.length === 0;

  if (empty) {
    return (
      <p className="pl-12 pr-4 py-1.5 text-[11px] text-muted-foreground/60">
        {t("route.specs.children_empty", "Sem ondas, ACs ou sub-specs.")}
      </p>
    );
  }

  return (
    <div className="flex flex-col pb-1">
      {data.waves.map((w) => (
        <SpecChildRow
          key={`w-${w.idx}`}
          kind="wave"
          label={w.role ? `${w.idx} · ${w.role}` : String(w.idx)}
          status={w.status}
          onClick={() => onOpenParent(spec)}
        />
      ))}
      {data.acs.map((ac) => (
        <SpecChildRow
          key={`ac-${ac.id}`}
          kind="ac"
          label={ac.id}
          detail={ac.label}
          status={ac.status}
          onClick={() => onOpenParent(spec)}
        />
      ))}
      {data.subspecs.map((s) => (
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
