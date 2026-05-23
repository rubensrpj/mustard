/**
 * AmendMetricsCard — telemetry card for amend-window metrics.
 * Wave 4, spec 2026-05-20-session-bound-amendments, AC-16.
 *
 * Calls 4 Tauri commands via dashboard.ts wrappers (never invoke() directly).
 * Dark-first, mustard-yellow title, indigo/violet accent, Inter font.
 */

import { useQuery } from "@tanstack/react-query";
import { useStore } from "@/lib/store";
import { useProjects } from "@/lib/dashboard";
import {
  fetchAmendResolutionRate,
  fetchAmendDriftRate,
  fetchCrossSessionAmendCount,
  fetchAmendWindowDuration,
} from "@/lib/dashboard";
import { cn } from "@/lib/utils";
import { Card, CardContent } from "@/components/ui/card";

// ── helpers ─────────────────────────────────────────────────────────────────

function avgMs(durations: number[]): number | null {
  if (durations.length === 0) return null;
  return Math.round(durations.reduce((a, b) => a + b, 0) / durations.length);
}

function fmtPct(v: number): string {
  return `${(v * 100).toFixed(1)}%`;
}

function fmtMs(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${(ms / 60_000).toFixed(1)} min`;
}

// ── Skeleton ─────────────────────────────────────────────────────────────────

function SkeletonTile() {
  return (
    <div className="flex flex-col gap-1.5 p-3 rounded-lg bg-muted/20 animate-pulse">
      <div className="h-3 w-20 bg-muted/50 rounded" />
      <div className="h-6 w-12 bg-muted/60 rounded" />
    </div>
  );
}

// ── Empty state ───────────────────────────────────────────────────────────────

function AmendEmptyState() {
  return (
    <div className="px-4 py-5 text-[13px] text-muted-foreground leading-relaxed">
      Nenhuma janela amend ainda registrada. Quando uma pipeline fechar nesta
      sessão, a próxima edição pós-CLOSE abrirá uma janela aqui.
    </div>
  );
}

// ── Metric tile ────────────────────────────────────────────────────────────────

function MetricTile({
  label,
  value,
  accent,
}: {
  label: string;
  value: string;
  accent?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex flex-col gap-1 p-3 rounded-lg border",
        accent
          ? "border-[--color-accent-mustard]/30 bg-[--color-accent-mustard]/5"
          : "border-border/40 bg-muted/10",
      )}
    >
      <span className="text-[11px] tracking-wide font-medium text-muted-foreground uppercase">
        {label}
      </span>
      <span
        className={cn(
          "text-xl font-mono font-medium tabular-nums",
          accent ? "text-[--color-accent-mustard]" : "text-foreground",
        )}
      >
        {value}
      </span>
    </div>
  );
}

// ── AmendMetricsCard ──────────────────────────────────────────────────────────

export function AmendMetricsCard() {
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const projects = useProjects();
  const path = projects.find((p) => p.id === activeWorkspaceId)?.path ?? null;

  const resolutionRate = useQuery({
    queryKey: ["amend-resolution-rate", path],
    queryFn: () => fetchAmendResolutionRate(path!),
    enabled: !!path,
    staleTime: 30_000,
  });

  const driftRate = useQuery({
    queryKey: ["amend-drift-rate", path],
    queryFn: () => fetchAmendDriftRate(path!),
    enabled: !!path,
    staleTime: 30_000,
  });

  const crossSession = useQuery({
    queryKey: ["amend-cross-session", path],
    queryFn: () => fetchCrossSessionAmendCount(path!),
    enabled: !!path,
    staleTime: 30_000,
  });

  const durations = useQuery({
    queryKey: ["amend-window-duration", path],
    queryFn: () => fetchAmendWindowDuration(path!),
    enabled: !!path,
    staleTime: 30_000,
  });

  const isLoading =
    resolutionRate.isLoading ||
    driftRate.isLoading ||
    crossSession.isLoading ||
    durations.isLoading;

  const rr = resolutionRate.data ?? 0;
  const dr = driftRate.data ?? 0;
  const cs = crossSession.data ?? 0;
  const durs = durations.data ?? [];
  const avg = avgMs(durs);

  const allZero = rr === 0 && dr === 0 && cs === 0 && avg === null;

  return (
    <Card size="sm" className="ring-foreground/5">
      <CardContent className="flex flex-col gap-3 pt-4">
        {/* Title */}
        <div className="flex flex-col gap-0.5">
          <h3
            className="text-[13px] font-medium tracking-tight"
            style={{ color: "var(--color-accent-mustard, #f5a623)" }}
          >
            Janelas de emenda
          </h3>
          <p className="text-[11px] text-muted-foreground/70">
            Amend windows — métricas pós-CLOSE desta sessão
          </p>
        </div>

        {/* Content */}
        {isLoading ? (
          <div className="grid grid-cols-2 gap-2">
            <SkeletonTile />
            <SkeletonTile />
            <SkeletonTile />
            <SkeletonTile />
          </div>
        ) : allZero ? (
          <AmendEmptyState />
        ) : (
          <div className="grid grid-cols-2 gap-2">
            <MetricTile
              label="Taxa de resolução"
              value={fmtPct(rr)}
              accent
            />
            <MetricTile
              label="Taxa de drift"
              value={fmtPct(dr)}
            />
            <MetricTile
              label="Pendentes cross-session"
              value={String(cs)}
            />
            <MetricTile
              label="Duração média"
              value={avg !== null ? fmtMs(avg) : "—"}
              accent
            />
          </div>
        )}

        {/* Histogram preview — show sample count when durations present */}
        {!isLoading && !allZero && durs.length > 0 && (
          <p className="text-[11px] text-muted-foreground/60">
            {durs.length} janela{durs.length !== 1 ? "s" : ""} medida
            {durs.length !== 1 ? "s" : ""}
          </p>
        )}
      </CardContent>
    </Card>
  );
}
