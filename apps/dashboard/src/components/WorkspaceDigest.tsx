import { useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router";
import { RefreshCw, ArrowUpRight } from "lucide-react";
import type { Project } from "@/api/discovery";
import {
  fetchActivePipelines,
  fetchActivityAggregated,
  fetchConsumption,
  fetchMetrics,
  fetchRecentEvents,
  fetchSpecs,
  type ActivityGroup,
  type DailyPoint,
} from "@/lib/dashboard";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";
import { parseQaOverall } from "@/lib/qa";

const POLL_MS = 15_000;
const FRESH_MS = 5 * 60_000;

interface Props {
  project: Project;
}

function compactNumber(n: number): string {
  if (!Number.isFinite(n)) return "0";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n >= 10_000 ? 0 : 1)}k`;
  return n.toLocaleString();
}

function isToday(iso: string | null | undefined): boolean {
  if (!iso) return false;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return false;
  const now = new Date();
  return (
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate()
  );
}

function topGroup(groups: ActivityGroup[] | undefined): ActivityGroup | null {
  if (!groups || groups.length === 0) return null;
  let best: ActivityGroup | null = null;
  for (const g of groups) {
    if (!g.spec) continue;
    if (!best || g.tokens_total > best.tokens_total) best = g;
  }
  return best;
}

function specShortName(s: string): string {
  return s.replace(/^\d{4}-\d{2}-\d{2}-/, "");
}

interface DigestStat {
  label: string;
  value: string;
  hint?: string;
  tone?: "default" | "muted";
}

function StatCard({ label, value, hint, tone = "default" }: DigestStat) {
  return (
    <div className="border border-border rounded-md p-3 flex flex-col gap-0.5 bg-card/30">
      <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
      <span
        className={`text-xl font-semibold tabular-nums ${tone === "muted" ? "text-muted-foreground" : "text-foreground"}`}
      >
        {value}
      </span>
      {hint && (
        <span className="text-[11px] text-muted-foreground truncate" title={hint}>
          {hint}
        </span>
      )}
    </div>
  );
}

function HeatBars({ daily }: { daily: DailyPoint[] }) {
  const series = useMemo(() => daily.slice(-7), [daily]);
  const max = useMemo(() => Math.max(1, ...series.map((d) => d.total_tokens)), [series]);
  if (series.length === 0) {
    return (
      <p className="text-[12px] text-muted-foreground">Sem dados nos últimos 7 dias.</p>
    );
  }
  return (
    <div className="flex items-end gap-1 h-12">
      {series.map((d) => {
        const h = Math.max(2, Math.round((d.total_tokens / max) * 100));
        const dayLabel = new Date(d.date).toLocaleDateString(undefined, {
          weekday: "short",
        });
        const title = `${d.date}: ${compactNumber(d.total_tokens)} tok · ${d.calls} chamadas`;
        return (
          <div key={d.date} className="flex flex-col items-center gap-1 flex-1 min-w-0" title={title}>
            <div
              className="w-full rounded-sm bg-primary/70"
              style={{ height: `${h}%` }}
              aria-label={title}
            />
            <span className="text-[9px] text-muted-foreground uppercase font-mono">
              {dayLabel.slice(0, 3)}
            </span>
          </div>
        );
      })}
    </div>
  );
}

export function WorkspaceDigest({ project }: Props) {
  const queryClient = useQueryClient();

  const { data: metrics } = useQuery({
    queryKey: ["metrics", project.path],
    queryFn: () => fetchMetrics(project.path),
    staleTime: 10_000,
    refetchInterval: POLL_MS,
  });

  const { data: pipelines } = useQuery({
    queryKey: ["active-pipelines", project.path],
    queryFn: () => fetchActivePipelines(project.path),
    staleTime: 5_000,
    refetchInterval: POLL_MS,
  });

  const { data: specs } = useQuery({
    queryKey: ["specs", project.path],
    queryFn: () => fetchSpecs(project.path),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  const { data: agg } = useQuery({
    queryKey: ["activity-agg", project.path],
    queryFn: () => fetchActivityAggregated(project.path, 200),
    staleTime: 15_000,
    refetchInterval: 30_000,
  });

  const { data: consumption } = useQuery({
    queryKey: ["consumption", project.path],
    queryFn: () => fetchConsumption(project.path),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });

  const { data: recent, dataUpdatedAt: recentUpdatedAt, refetch: refetchRecent } = useQuery({
    queryKey: ["recent-events", project.path, 5],
    queryFn: () => fetchRecentEvents(project.path, 5),
    staleTime: 5_000,
    refetchInterval: 10_000,
  });

  // Wider window strictly for QA verdicts today; 200 events covers most days
  // without bloating the response. Separate from `recent` (which feeds the
  // pulse row and stays small for snappy updates).
  const { data: qaEvents } = useQuery({
    queryKey: ["recent-events-qa-today", project.path],
    queryFn: () => fetchRecentEvents(project.path, 200),
    staleTime: 15_000,
    refetchInterval: 30_000,
  });

  const completedToday = useMemo(
    () =>
      (specs ?? []).filter(
        (s) => s.bucket === "completed" && isToday(s.completed_at),
      ).length,
    [specs],
  );

  const qaToday = useMemo(() => {
    const c = { pass: 0, fail: 0, skip: 0 };
    for (const e of qaEvents ?? []) {
      if (e.event_type !== "qa.result") continue;
      if (!isToday(e.ts)) continue;
      const o = parseQaOverall(e.summary);
      if (!o) continue;
      c[o]++;
    }
    return c;
  }, [qaEvents]);

  const qaTotalToday = qaToday.pass + qaToday.fail + qaToday.skip;
  const qaPassRateToday = qaTotalToday > 0 ? qaToday.pass / qaTotalToday : 0;

  const lastEventTs = metrics?.last_event_at ?? recent?.[0]?.ts ?? null;
  const lastEventMs = lastEventTs ? Date.parse(lastEventTs) : null;
  const isFresh = lastEventMs ? Date.now() - lastEventMs < FRESH_MS : false;
  const top = topGroup(agg);
  const activeCount = pipelines?.length ?? 0;
  const tokensToday = metrics?.tokens_today ?? 0;

  function refreshAll() {
    queryClient.invalidateQueries({ queryKey: ["metrics", project.path] });
    queryClient.invalidateQueries({ queryKey: ["active-pipelines", project.path] });
    queryClient.invalidateQueries({ queryKey: ["specs", project.path] });
    queryClient.invalidateQueries({ queryKey: ["activity-agg", project.path] });
    queryClient.invalidateQueries({ queryKey: ["consumption", project.path] });
    refetchRecent();
  }

  return (
    <section className="flex flex-col gap-3">
      {/* Pulse row */}
      <div className="flex items-center gap-3 px-3 py-2 border border-border rounded-md bg-card/30">
        <span
          className={`inline-block w-2 h-2 rounded-full ${
            isFresh ? "bg-[--color-ok] animate-pulse ring-1 ring-[--color-ok]/30" : "bg-zinc-500"
          }`}
          aria-hidden
        />
        <div className="flex flex-col leading-tight">
          <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
            {isFresh ? "Ao vivo" : "Em repouso"}
          </span>
          <span className="text-[13px] text-foreground">
            {lastEventTs
              ? `Última atividade ${relativeTime(lastEventTs)}`
              : "Sem eventos registrados"}
          </span>
        </div>
        <span className="ml-auto text-[11px] text-muted-foreground font-mono">
          {recentUpdatedAt
            ? `lido ${relativeTime(new Date(recentUpdatedAt).toISOString())}`
            : ""}
        </span>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={refreshAll}
          aria-label="Atualizar"
        >
          <RefreshCw className="h-3.5 w-3.5" />
        </Button>
      </div>

      {/* Stat grid */}
      <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-5 gap-2">
        <StatCard
          label="Em progresso"
          value={String(activeCount)}
          hint={activeCount > 0 ? "pipelines ativos" : "nenhum pipeline"}
        />
        <StatCard
          label="Concluídas hoje"
          value={String(completedToday)}
          hint={completedToday > 0 ? "specs finalizadas" : "—"}
          tone={completedToday === 0 ? "muted" : "default"}
        />
        <StatCard
          label="QA pass-rate hoje"
          value={qaTotalToday > 0 ? `${Math.round(qaPassRateToday * 100)}%` : "—"}
          hint={
            qaTotalToday > 0
              ? `${qaToday.pass}✓ ${qaToday.fail}✗${qaToday.skip > 0 ? ` ${qaToday.skip}⊘` : ""}`
              : "sem qa.result hoje"
          }
          tone={qaTotalToday === 0 ? "muted" : "default"}
        />
        <StatCard
          label="Tokens hoje"
          value={compactNumber(tokensToday)}
          hint={
            consumption?.cost_today_usd
              ? `~ US$ ${consumption.cost_today_usd.toFixed(2)}`
              : undefined
          }
          tone={tokensToday === 0 ? "muted" : "default"}
        />
        <StatCard
          label="Eventos (total)"
          value={compactNumber(metrics?.total_events ?? 0)}
          hint={`${metrics?.sessions_recent ?? 0} sessões recentes`}
        />
      </div>

      {/* Focus + 7-day */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
        <div className="border border-border rounded-md p-3 flex flex-col gap-1 bg-card/30">
          <div className="flex items-baseline gap-2">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Foco do dia
            </span>
            {top && (
              <Badge variant="secondary" className="text-[10px] py-0 font-mono">
                W{top.wave ?? "—"}
              </Badge>
            )}
          </div>
          {top ? (
            <Link
              to={`/project/${project.id}/spec/${encodeURIComponent(top.spec ?? "")}`}
              className="flex items-center gap-1 text-sm font-mono text-foreground hover:underline"
            >
              {specShortName(top.spec ?? "")}
              <ArrowUpRight className="h-3 w-3 opacity-60" />
            </Link>
          ) : (
            <span className="text-sm text-muted-foreground">Nenhuma spec ativa hoje.</span>
          )}
          {top && (
            <div className="flex items-baseline gap-3 text-[11px] text-muted-foreground font-mono">
              <span>{top.count} ações</span>
              <span>{compactNumber(top.tokens_total)} tok</span>
              <span>{top.files_touched} arquivos</span>
            </div>
          )}
        </div>

        <div className="border border-border rounded-md p-3 flex flex-col gap-2 bg-card/30">
          <div className="flex items-baseline justify-between">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Últimos 7 dias
            </span>
            <Link
              to="/telemetry"
              className="text-[10px] text-muted-foreground hover:text-foreground"
            >
              ver telemetria →
            </Link>
          </div>
          <HeatBars daily={consumption?.daily_series ?? []} />
        </div>
      </div>

      <div className="flex items-baseline gap-2 mt-1">
        <Link
          to="/activity"
          className="text-[12px] text-muted-foreground hover:text-foreground"
        >
          Ver atividade detalhada →
        </Link>
      </div>
    </section>
  );
}
