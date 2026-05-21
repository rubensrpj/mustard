import { useMemo } from "react";
import { useNavigate } from "react-router";
import { CheckCircle2, ClipboardCheck, Eye, Ban, Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { DataCard, SectionHeader } from "@/components/page";
import { useWorkspaceSummarySingle } from "@/hooks/useWorkspaceSummary";
import { useTranslate } from "@/lib/i18n";
import type { SpecTrack } from "@/lib/types/specs";

interface WorkspaceStatusCountersProps {
  repoPath: string;
}

type CounterKey = "completed" | "qa" | "reviewing" | "implementing" | "blocked";

/**
 * Map a raw `SpecTrack.status` into one of the counter buckets. Matches the
 * bucketing in `WorkspaceSpecsByStatus` but uses a wider set of buckets so the
 * counters surface "blocked" separately from "pending".
 */
function bucketFor(status: string): CounterKey | null {
  const s = status.toLowerCase();
  if (s.includes("complete") || s === "done" || s === "closed") return "completed";
  if (s.includes("qa") || s.includes("await")) return "qa";
  if (s.includes("review")) return "reviewing";
  if (s.includes("block") || s === "wave-failed") return "blocked";
  if (s === "no-events" || s === "draft" || s === "planning" || s === "cancelled") return null;
  // EXECUTE / implementing / running.
  return "implementing";
}

interface CounterTile {
  key: CounterKey;
  label: string;
  count: number;
  icon: typeof CheckCircle2;
  tone: string;
  navigateTo: string;
}

export function WorkspaceStatusCounters({ repoPath }: WorkspaceStatusCountersProps) {
  const t = useTranslate();
  const navigate = useNavigate();
  const { data, isLoading } = useWorkspaceSummarySingle(repoPath);

  const counts: Record<CounterKey, number> = useMemo(() => {
    const result: Record<CounterKey, number> = {
      completed: 0,
      qa: 0,
      reviewing: 0,
      implementing: 0,
      blocked: 0,
    };
    const tracks: SpecTrack[] = data?.spec_tracks ?? [];
    for (const track of tracks) {
      const bucket = bucketFor(track.status);
      if (bucket) result[bucket] += 1;
    }
    return result;
  }, [data?.spec_tracks]);

  // Tile palette — `tone` resolves to a Tailwind text color over the shared
  // DS surface; keeping the mapping inline lets the file stay self-contained.
  const tiles: CounterTile[] = [
    {
      key: "completed",
      label: t("status.completed"),
      count: counts.completed,
      icon: CheckCircle2,
      tone: "text-[--ds-status-completed]",
      navigateTo: "/specs?status=completed",
    },
    {
      key: "qa",
      label: t("status.qa"),
      count: counts.qa,
      icon: ClipboardCheck,
      tone: "text-[--ds-status-awaiting-qa]",
      navigateTo: "/specs?status=qa",
    },
    {
      key: "reviewing",
      label: t("status.reviewing"),
      count: counts.reviewing,
      icon: Eye,
      tone: "text-[--ds-accent-primary]",
      navigateTo: "/specs?status=reviewing",
    },
    {
      key: "implementing",
      label: t("status.implementing"),
      count: counts.implementing,
      icon: Loader2,
      tone: "text-[--ds-status-implementing]",
      navigateTo: "/specs?status=implementing",
    },
    {
      key: "blocked",
      label: t("status.blocked"),
      count: counts.blocked,
      icon: Ban,
      tone: "text-[--ds-intent-error]",
      navigateTo: "/specs?status=blocked",
    },
  ];

  return (
    <DataCard padded>
      <SectionHeader title={t("workspace.statusCounters")} />
      {isLoading && !data ? (
        <p className="mt-3 text-[12.5px] text-muted-foreground/70">{t("common.loading")}</p>
      ) : (
        <ul
          className="mt-3 grid grid-cols-5 gap-3"
          aria-label={t("workspace.statusCounters")}
        >
          {tiles.map((tile) => {
            const Icon = tile.icon;
            return (
              <li key={tile.key}>
                <button
                  type="button"
                  onClick={() => navigate(tile.navigateTo)}
                  className={cn(
                    "w-full flex flex-col items-center justify-center gap-1.5",
                    "rounded-md border border-border/60 bg-card/40",
                    "px-2 py-3 hover:bg-muted/40 transition-colors",
                    "focus-visible:outline-none focus-visible:ring-2",
                    "focus-visible:ring-[--ds-accent-primary]/60",
                  )}
                  aria-label={`${tile.label}: ${tile.count}`}
                >
                  <Icon
                    className={cn("h-4 w-4", tile.tone)}
                    aria-hidden
                  />
                  <span
                    className="text-2xl font-semibold tabular-nums text-foreground"
                    style={{ fontVariantNumeric: "tabular-nums" }}
                  >
                    {tile.count}
                  </span>
                  <span className="text-[11px] text-muted-foreground uppercase tracking-wide">
                    {tile.label}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </DataCard>
  );
}
