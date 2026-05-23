import { useState, useMemo } from "react";
import { useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { fetchWorkspaceHealth } from "@/lib/dashboard";
import { relativeTime } from "@/lib/time";
import type { WorkspaceHealth } from "@/lib/types/specs";

/**
 * Wave-6 (2026-05-21, spec `spec-lifecycle-unification/wave-6-observability`) —
 * hygiene health card rendered on `/workspace` directly below `<WorkspaceHero>`.
 *
 * Shows 5 clickable numeric counters:
 *   Ativas · Suspeitas · Auto-fechadas hoje · Bloqueadas · Wave failed
 *
 * Each counter links to `/specs?filter=<key>`. Collapsible:
 *   - Default expanded when any signal counter > 0 (`suspects`, `autoclose_today`,
 *     `blocked`, or `wave_failed`).
 *   - Default collapsed when everything is zero (no hygiene noise).
 *
 * Live-updates via a 60s fallback `refetchInterval` (no dedicated watcher kind).
 */

interface WorkspaceHealthCardProps {
  repoPath: string;
}

interface CounterProps {
  label: string;
  value: number;
  filterKey: string;
  hasSignal?: boolean;
}

function Counter({ label, value, filterKey, hasSignal }: CounterProps) {
  const navigate = useNavigate();

  function handleClick() {
    navigate(`/specs?filter=${filterKey}`);
  }

  return (
    <button
      type="button"
      onClick={handleClick}
      className={cn(
        "flex flex-col items-center gap-0.5 px-4 py-2 rounded-md transition-colors",
        "hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
        "min-w-[80px]",
      )}
      title={`Filtrar por: ${label}`}
    >
      <span
        className={cn(
          "text-2xl font-semibold tabular-nums leading-tight",
          hasSignal && value > 0
            ? "text-amber-400"
            : "text-foreground/80",
        )}
      >
        {value}
        {hasSignal && value > 0 && (
          <span className="ml-0.5 text-sm text-amber-400" aria-hidden>
            ▴
          </span>
        )}
      </span>
      <span className="text-[11px] text-muted-foreground leading-tight text-center whitespace-nowrap">
        {label}
      </span>
    </button>
  );
}

function LastRunLabel({ ts }: { ts: string | null }) {
  const t = useT();
  if (!ts) return null;

  const relative = relativeTime(ts);
  if (!relative) return null;

  // Replace the `{time}` placeholder in the translation template.
  const template = t("workspace.health.last_run", "Última verificação há {time}");
  const label = template.replace("{time}", relative);

  return (
    <span className="text-[11px] text-muted-foreground/70 shrink-0 whitespace-nowrap">
      {label}
    </span>
  );
}

function HealthContent({ health }: { health: WorkspaceHealth }) {
  const t = useT();
  return (
    <div className="flex flex-wrap items-center gap-1">
      <Counter
        label={t("workspace.health.active", "Ativas")}
        value={health.active}
        filterKey="ativas"
      />
      <Counter
        label={t("workspace.health.suspects", "Suspeitas")}
        value={health.suspects}
        filterKey="suspects"
        hasSignal
      />
      <Counter
        label={t("workspace.health.autoclose_today", "Auto-fechadas hoje")}
        value={health.autoclose_today}
        filterKey="autoclose"
        hasSignal
      />
      <Counter
        label={t("workspace.health.blocked", "Bloqueadas")}
        value={health.blocked}
        filterKey="blocked"
        hasSignal
      />
      <Counter
        label={t("workspace.health.wave_failed", "Wave failed")}
        value={health.wave_failed}
        filterKey="wave-failed"
        hasSignal
      />
    </div>
  );
}

export function WorkspaceHealthCard({ repoPath }: WorkspaceHealthCardProps) {
  const t = useT();

  const { data: health } = useQuery({
    queryKey: ["workspace-health", repoPath],
    queryFn: () => fetchWorkspaceHealth(repoPath),
    enabled: !!repoPath,
    staleTime: 10_000,
    // No dedicated watcher kind — long 60s fallback (Wave 3, 2026-05-22).
    refetchInterval: 60_000,
    refetchIntervalInBackground: false,
  });

  // Default expanded when any signal counter > 0.
  const hasSignal = useMemo(
    () =>
      (health?.suspects ?? 0) > 0 ||
      (health?.autoclose_today ?? 0) > 0 ||
      (health?.blocked ?? 0) > 0 ||
      (health?.wave_failed ?? 0) > 0,
    [health],
  );

  const [expanded, setExpanded] = useState<boolean | null>(null);
  // On first data arrival, set the default based on signal.
  const resolvedExpanded = expanded ?? hasSignal;

  const Chevron = resolvedExpanded ? ChevronDown : ChevronRight;

  return (
    <div className="rounded-lg border border-border bg-card/60 overflow-hidden">
      {/* Header row */}
      <button
        type="button"
        onClick={() => setExpanded(!resolvedExpanded)}
        className={cn(
          "w-full flex items-center justify-between gap-3 px-4 py-2.5",
          "text-left transition-colors hover:bg-muted/20",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
        )}
        aria-expanded={resolvedExpanded}
      >
        <div className="flex items-center gap-2">
          <Chevron className="h-3.5 w-3.5 text-muted-foreground/60 shrink-0" aria-hidden />
          <span className="text-[13px] font-medium text-foreground/80">
            {t("workspace.health.title", "Saúde do workspace")}
          </span>
          {hasSignal && (
            <span
              className="h-1.5 w-1.5 rounded-full bg-amber-400 shrink-0"
              title="Há specs que precisam de atenção"
              aria-hidden
            />
          )}
        </div>
        {health && <LastRunLabel ts={health.last_hygiene_run_at} />}
      </button>

      {/* Collapsible body */}
      {resolvedExpanded && (
        <div className="px-3 pb-3 border-t border-border/40">
          {health ? (
            <HealthContent health={health} />
          ) : (
            <p className="text-[12px] text-muted-foreground py-2 px-1">
              {t("common.loading", "Carregando…")}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
