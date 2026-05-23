import { useMemo, useState } from "react";
import { useNavigate } from "react-router";
import { cn } from "@/lib/utils";
import { DataCard, SectionHeader, EmptyState } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { useWorkspaceSummarySingle } from "@/hooks/useWorkspaceSummary";
import type { SpecTrack } from "@/lib/types/specs";

interface WorkspaceSpecsByStatusProps {
  repoPath: string;
}

type Period = "Hoje" | "7d" | "30d";
const PERIODS: Period[] = ["Hoje", "7d", "30d"];

const PERIOD_WINDOW_MS: Record<Period, number> = {
  Hoje: 24 * 60 * 60 * 1000,
  "7d": 7 * 24 * 60 * 60 * 1000,
  "30d": 30 * 24 * 60 * 60 * 1000,
};

type StatusKey =
  | "status-draft"
  | "status-implementing"
  | "status-awaiting-qa"
  | "status-completed";

const STATUS_ORDER: StatusKey[] = [
  "status-draft",
  "status-implementing",
  "status-awaiting-qa",
  "status-completed",
];

const STATUS_LABEL: Record<StatusKey, string> = {
  "status-draft": "draft",
  "status-implementing": "implementing",
  "status-awaiting-qa": "awaiting-qa",
  "status-completed": "completed",
};

/**
 * Map a raw `SpecTrack.status` value into one of the four canonical buckets
 * shown in the Visão Geral card. Keeps the visual surface stable even as
 * backend status strings drift.
 */
function bucketFor(track: SpecTrack): StatusKey {
  const s = (track.status ?? "").toLowerCase();
  if (s.includes("complete") || s === "done") return "status-completed";
  if (s.includes("qa") || s.includes("await")) return "status-awaiting-qa";
  if (s.includes("draft") || s.includes("plan") || s.includes("analyz"))
    return "status-draft";
  // EXECUTE / in_progress / running / anything else still active
  return "status-implementing";
}

function withinPeriod(track: SpecTrack, period: Period, now: number): boolean {
  if (!track.last_event_at) return false;
  const ts = Date.parse(track.last_event_at);
  if (Number.isNaN(ts)) return false;
  return now - ts <= PERIOD_WINDOW_MS[period];
}

/**
 * Spec count broken down by canonical status, with a Hoje | 7d | 30d window
 * filter. Consumes `useWorkspaceSummarySingle` and derives buckets purely on
 * the client — no extra round-trip.
 */
export function WorkspaceSpecsByStatus({ repoPath }: WorkspaceSpecsByStatusProps) {
  const navigate = useNavigate();
  const [period, setPeriod] = useState<Period>("Hoje");
  const { data, isLoading } = useWorkspaceSummarySingle(repoPath);

  const counts = useMemo(() => {
    const result: Record<StatusKey, number> = {
      "status-draft": 0,
      "status-implementing": 0,
      "status-awaiting-qa": 0,
      "status-completed": 0,
    };
    if (!data?.spec_tracks) return result;
    const now = Date.now();
    for (const track of data.spec_tracks) {
      if (!withinPeriod(track, period, now)) continue;
      result[bucketFor(track)] += 1;
    }
    return result;
  }, [data?.spec_tracks, period]);

  const total = STATUS_ORDER.reduce((acc, k) => acc + counts[k], 0);

  const segmented = (
    <div
      className="inline-flex rounded-md border border-border bg-card/30 p-0.5"
      role="tablist"
      aria-label="Janela de tempo"
    >
      {PERIODS.map((p) => (
        <button
          key={p}
          type="button"
          role="tab"
          aria-selected={p === period}
          onClick={() => setPeriod(p)}
          className={cn(
            "px-2 py-0.5 text-[11px] rounded transition-colors",
            "focus-visible:outline-none focus-visible:ring-2",
            "focus-visible:ring-[--color-accent-mustard]",
            p === period
              ? "bg-foreground/10 text-foreground"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          {p}
        </button>
      ))}
    </div>
  );

  return (
    <DataCard padded>
      <SectionHeader title="Specs por status" right={segmented} />

      {isLoading && !data ? (
        <p className="mt-3 text-[12.5px] text-muted-foreground/70">Carregando…</p>
      ) : total === 0 ? (
        <EmptyState
          className="mt-3"
          title="Nenhuma spec na janela selecionada"
          description="Inicie uma pipeline ou amplie o período acima."
        />
      ) : (
        <ul className="mt-3 grid grid-cols-2 gap-2">
          {STATUS_ORDER.map((key) => (
            <li
              key={key}
              className={cn(
                "flex items-center justify-between gap-2 rounded-md border",
                "border-border/60 bg-card/30 px-2.5 py-2",
              )}
            >
              <Badge variant={key}>{STATUS_LABEL[key]}</Badge>
              <span
                className="text-lg font-semibold tabular-nums text-foreground"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                {counts[key]}
              </span>
            </li>
          ))}
        </ul>
      )}

      <div className="mt-3 text-right">
        <a
          href="/specs"
          onClick={(e) => {
            e.preventDefault();
            navigate("/specs");
          }}
          className={cn(
            "text-[11px] text-[--color-accent-mustard] hover:underline",
            "focus-visible:outline-none focus-visible:ring-2",
            "focus-visible:ring-[--color-accent-mustard] rounded",
          )}
        >
          Ver detalhes →
        </a>
      </div>
    </DataCard>
  );
}
