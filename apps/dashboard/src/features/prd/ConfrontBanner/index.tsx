import { Badge } from '@/components/ui/badge';
import type { PrdConfront } from '@/lib/types/prd';

/**
 * Amber alert block that surfaces, after a successful lapidate run,
 * which entities/paths the model hallucinated relative to the project's
 * real registry. Extracted from `IntentHero.tsx` (Wave 3-polish,
 * spec 2026-05-21-prd-lapidator-polish).
 *
 * Returns `null` when there is nothing to warn about, so the caller can
 * unconditionally render `<ConfrontBanner confront={confront} />`.
 */
export interface ConfrontBannerProps {
  confront: PrdConfront | null;
}

export function ConfrontBanner({ confront }: ConfrontBannerProps) {
  if (
    !confront ||
    (confront.entitiesMissing.length === 0 && confront.pathsMissing.length === 0)
  ) {
    return null;
  }

  return (
    <div
      role="alert"
      className="flex flex-col gap-1.5 border border-amber-400/40 bg-amber-50/40 dark:bg-amber-500/5 rounded px-3 py-2"
    >
      {confront.entitiesMissing.length > 0 && (
        <div className="flex items-center gap-1.5 flex-wrap text-xs">
          <span className="text-amber-700 dark:text-amber-300 font-medium">
            Entidades ausentes no registry:
          </span>
          {confront.entitiesMissing.map((e) => (
            <Badge key={e} variant="warning">
              {e}
            </Badge>
          ))}
        </div>
      )}
      {confront.pathsMissing.length > 0 && (
        <div className="flex items-center gap-1.5 flex-wrap text-xs">
          <span className="text-amber-700 dark:text-amber-300 font-medium">
            Paths não encontrados:
          </span>
          {confront.pathsMissing.map((p) => (
            <Badge key={p} variant="warning" className="font-mono">
              {p}
            </Badge>
          ))}
        </div>
      )}
    </div>
  );
}
