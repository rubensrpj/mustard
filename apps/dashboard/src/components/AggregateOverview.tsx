import { useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { Activity, FolderGit2, Layers, Play, CheckCircle2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import {
  useAggregate,
  type ActivePipelineRow,
  type TimelineRow,
} from "@/hooks/useAggregate";
import type { Project } from "@/api/discovery";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { relativeTime } from "@/lib/time";
import {
  fetchConsumptionGlobal,
  type ModelUsage,
  type ProjectUsage,
  type DailyPoint,
  type GlobalConsumption,
} from "@/lib/dashboard";
import { useStore } from "@/lib/store";
import { formatTokens, formatUsd, formatPct, formatNumber } from "@/lib/format";

function specVariant(phase: string | null, status: string | null): StatusDotVariant {
  if (status === "blocked") return "blocked";
  switch (phase) {
    case "EXECUTE":
      return "active";
    case "ANALYZE":
    case "PLAN":
    case "QA":
      return "planning";
    case "CLOSE":
      return "done";
    default:
      return "idle";
  }
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

function KpiCard({
  label,
  value,
  sub,
  loading,
}: {
  label: string;
  value: string;
  sub?: string;
  loading: boolean;
}) {
  return (
    <div className="flex flex-col gap-1 px-3 py-2 rounded border border-border bg-card">
      <div className="text-[11px] uppercase tracking-wider text-muted-foreground">{label}</div>
      <span className="text-xl font-mono font-medium text-foreground">{loading ? "—" : value}</span>
      {sub && <span className="text-[11px] text-muted-foreground/80">{sub}</span>}
    </div>
  );
}

function AggregateSparkline({
  series,
  rtkSeries,
}: {
  series: DailyPoint[];
  rtkSeries: { date: string; saved_tokens: number }[];
}) {
  // Align onto a 14-day window ending today.
  const days: string[] = (() => {
    const out: string[] = [];
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    for (let i = 13; i >= 0; i--) {
      const d = new Date(today.getTime() - i * 86_400_000);
      out.push(d.toISOString().slice(0, 10));
    }
    return out;
  })();
  const consMap = new Map<string, number>();
  for (const p of series) consMap.set(p.date, p.total_tokens);
  const savedMap = new Map<string, number>();
  for (const p of rtkSeries) savedMap.set(p.date, p.saved_tokens);

  const points = days.map((date) => ({
    date,
    consumed: consMap.get(date) ?? 0,
    saved: savedMap.get(date) ?? 0,
  }));
  const max = Math.max(1, ...points.map((p) => Math.max(p.consumed, p.saved)));
  const W = 560;
  const H = 80;
  const padT = 4;
  const padB = 14;
  const chartH = H - padT - padB;
  const slotW = W / points.length;
  const path = (key: "consumed" | "saved") =>
    points
      .map((p, i) => {
        const x = i * slotW + slotW / 2;
        const y = padT + chartH - (p[key] / max) * chartH;
        return `${i === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
      })
      .join(" ");

  return (
    <div className="flex flex-col gap-1">
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full h-20">
        <path d={path("saved")} fill="none" stroke="var(--success)" strokeOpacity="0.8" strokeWidth="1.5" />
        <path d={path("consumed")} fill="none" stroke="var(--primary)" strokeWidth="1.5" />
      </svg>
      <div className="flex gap-3 text-[11px] text-muted-foreground">
        <span className="flex items-center gap-1.5">
          <span className="inline-block w-3 h-0.5 bg-primary" /> consumido
        </span>
        <span className="flex items-center gap-1.5">
          <span className="inline-block w-3 h-0.5 bg-success" /> RTK saved
        </span>
      </div>
    </div>
  );
}

function Counter({
  label,
  value,
  icon: Icon,
  loading,
}: {
  label: string;
  value: number;
  icon: LucideIcon;
  loading: boolean;
}) {
  return (
    <div className="flex flex-col gap-1 px-3 py-2 rounded border border-border bg-card">
      <div className="flex items-center gap-2 text-muted-foreground">
        <Icon className="h-3.5 w-3.5" />
        <span className="text-[11px] uppercase tracking-wider">{label}</span>
      </div>
      <span className="text-xl font-mono font-medium text-foreground">
        {loading ? "—" : value}
      </span>
    </div>
  );
}

/**
 * ROI scoreboard — answers the blunt question "does running Mustard pay off?".
 *
 * Honest framing, no invented pricing:
 *  - `tokens poupados` is measured by the RTK binary (`rtk gain`) — real
 *    compressed-command savings across every project.
 *  - the COM/SEM split is a counterfactual on tokens, not dollars: "sem o
 *    Mustard, estes tokens teriam ido para o modelo". The percentage is RTK's
 *    own measured efficiency.
 *  - USD measured by the Anthropic API is per-project (OTEL) and lives on the
 *    Telemetry → Economia tab; we link there instead of faking a portfolio sum.
 */
function RoiScoreboard({
  globalCons,
  loading,
}: {
  globalCons: GlobalConsumption | undefined;
  loading: boolean;
}) {
  const rtk = globalCons?.rtk;
  const saved = rtk?.tokens_saved ?? 0;
  const consumed = globalCons?.tokens_total ?? 0;
  // "SEM Mustard" = o que teria ido ao modelo sem a compressão = consumido + poupado.
  const withoutMustard = consumed + saved;
  const effPct = rtk?.savings_pct ?? null;
  const hasData = !!rtk?.available && saved > 0;

  return (
    <section className="flex flex-col gap-2">
      <div className="flex flex-col gap-0.5">
        <h2 className="text-xs uppercase tracking-wider font-medium text-foreground">
          Compensa usar o Mustard?
        </h2>
        <p className="text-[12px] text-muted-foreground/80 leading-snug">
          Comparação contrafactual de tokens: o que de fato foi para o modelo COM
          o Mustard, contra a estimativa SEM ele. Os tokens poupados são medidos
          pelo RTK (compressão de saída de comandos) — não é estimativa de preço.
        </p>
      </div>
      {loading && !globalCons ? (
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
          {[0, 1, 2].map((i) => (
            <div key={i} className="h-20 rounded border border-border bg-card animate-pulse" />
          ))}
        </div>
      ) : !hasData ? (
        <div className="rounded border border-border bg-card px-3 py-3 text-[12.5px] text-muted-foreground leading-relaxed">
          Ainda sem dados de economia. O RTK precisa estar instalado e ter
          comprimido pelo menos um comando.{" "}
          {rtk?.available === false && (
            <>
              Rode <code className="font-mono text-foreground">rtk init -g</code> para ativar.
            </>
          )}
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
          <div className="flex flex-col gap-1 px-3 py-2.5 rounded border border-emerald-500/30 bg-emerald-500/5">
            <span className="text-[10px] uppercase tracking-wider text-emerald-400">
              COM Mustard — foi ao modelo
            </span>
            <span className="text-xl font-mono font-medium text-foreground tabular-nums">
              {formatTokens(consumed)}
            </span>
            <span className="text-[11px] text-muted-foreground">tokens efetivamente enviados</span>
          </div>
          <div className="flex flex-col gap-1 px-3 py-2.5 rounded border border-border bg-card">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              SEM Mustard — estimativa
            </span>
            <span className="text-xl font-mono font-medium text-muted-foreground tabular-nums">
              {formatTokens(withoutMustard)}
            </span>
            <span className="text-[11px] text-muted-foreground">consumido + poupado pelo RTK</span>
          </div>
          <div className="flex flex-col gap-1 px-3 py-2.5 rounded border border-primary/30 bg-primary/5">
            <span className="text-[10px] uppercase tracking-wider text-primary">
              Diferença poupada
            </span>
            <span className="text-xl font-mono font-medium text-primary tabular-nums">
              {formatTokens(saved)}
              {effPct != null && (
                <span className="text-sm text-muted-foreground"> · {formatPct(effPct)}</span>
              )}
            </span>
            <span className="text-[11px] text-muted-foreground">
              tokens que o Mustard evitou de enviar
            </span>
          </div>
        </div>
      )}
      <p className="text-[11px] text-muted-foreground/60 leading-snug">
        Custo em USD é medido pela Anthropic API por projeto — veja em{" "}
        <span className="text-muted-foreground/80">Telemetria → Economia</span>.
        O custo agregado da seção abaixo é estimado (tokens × tabela de preço), não cobrado.
      </p>
    </section>
  );
}

export function AggregateOverview({ projects }: { projects: Project[] }) {
  const navigate = useNavigate();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const { counters, activePipelines, timeline, loading } = useAggregate(projects);

  const { data: globalCons, isLoading: consLoading } = useQuery({
    queryKey: ["consumption-global", projectsRoot],
    queryFn: () => fetchConsumptionGlobal(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  return (
    <div className="flex flex-col gap-6">
      <section>
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
          <Counter label="Specs ativas" value={counters.activeSpecs} icon={Layers} loading={loading} />
          <Counter label="Em EXECUTE" value={counters.executing} icon={Play} loading={loading} />
          <Counter label="Completed 7d" value={counters.completed7d} icon={CheckCircle2} loading={loading} />
          <Counter label="Eventos hoje" value={counters.eventsToday} icon={Activity} loading={loading} />
        </div>
      </section>

      {/* ── Placar de ROI — "compensa usar o Mustard?" ──────────────────── */}
      <RoiScoreboard globalCons={globalCons} loading={consLoading} />

      {/* ── Consumo & Economia agregado ──────────────────────────────────── */}
      <section className="flex flex-col gap-3">
        <h2 className="text-xs uppercase tracking-wider font-medium text-foreground">
          Consumo &amp; Economia — todos os projetos
        </h2>
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
          <KpiCard
            label="Tokens total"
            value={globalCons ? formatTokens(globalCons.tokens_total) : "—"}
            sub={globalCons ? `hoje ${formatTokens(globalCons.tokens_today)}` : undefined}
            loading={consLoading}
          />
          <KpiCard
            label="Custo USD (estimado)"
            value={globalCons ? formatUsd(globalCons.cost_total_usd) : "—"}
            sub={globalCons ? `hoje ${formatUsd(globalCons.cost_today_usd)} · tokens × tabela` : undefined}
            loading={consLoading}
          />
          <KpiCard
            label="RTK saved"
            value={globalCons?.rtk.tokens_saved != null ? formatTokens(globalCons.rtk.tokens_saved) : "—"}
            sub={
              globalCons?.rtk.savings_pct != null
                ? `${formatPct(globalCons.rtk.savings_pct)} efic. · global · vitalício`
                : "global · todos os projetos"
            }
            loading={consLoading}
          />
          <KpiCard
            label="RTK commands"
            value={globalCons?.rtk.total_commands != null ? formatNumber(globalCons.rtk.total_commands) : "—"}
            sub={globalCons?.rtk.available === false ? "rtk não instalado" : "global · todos os projetos"}
            loading={consLoading}
          />
        </div>

        {/* Sparkline 14d */}
        {globalCons && globalCons.daily_series.length > 0 && (
          <AggregateSparkline series={globalCons.daily_series} rtkSeries={globalCons.rtk.daily} />
        )}

        {/* Por modelo */}
        {globalCons && globalCons.by_model.length > 0 && (
          <div className="flex flex-col gap-1">
            <h3 className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground">
              Por modelo (todos os projetos)
            </h3>
            <ul className="flex flex-col gap-0.5">
              {globalCons.by_model.map((m: ModelUsage) => (
                <li key={m.model} className="flex items-baseline gap-2 text-[13px]">
                  <span className="font-mono w-36 truncate">{m.model}</span>
                  <div className="flex-1 h-1.5 bg-muted rounded overflow-hidden">
                    <div className="h-full bg-primary/40" style={{ width: `${m.pct_tokens * 100}%` }} />
                  </div>
                  <span className="text-muted-foreground text-xs w-16 text-right font-mono">
                    {formatTokens(m.total_tokens)}
                  </span>
                  <span className="text-xs w-16 text-right font-mono">
                    {formatUsd(m.cost_usd)}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}

        {/* Por projeto */}
        {globalCons && globalCons.by_project.length > 0 && (
          <div className="flex flex-col gap-1">
            <h3 className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground">
              Por projeto (ordenado por custo)
            </h3>
            <table className="w-full text-[13px]">
              <thead>
                <tr className="text-left text-[11px] uppercase text-muted-foreground">
                  <th className="pb-1">Projeto</th>
                  <th className="pb-1 text-right">Tokens</th>
                  <th className="pb-1 text-right">Hoje</th>
                  <th className="pb-1 text-right">Custo</th>
                  <th className="pb-1 text-right">Última atividade</th>
                </tr>
              </thead>
              <tbody>
                {globalCons.by_project.map((p: ProjectUsage) => (
                  <tr
                    key={p.id}
                    className="border-t border-border hover:bg-muted/40 cursor-pointer"
                    onClick={() => navigate(`/project/${p.id}`)}
                  >
                    <td className="py-1">{p.name}</td>
                    <td className="py-1 text-right font-mono">{formatTokens(p.tokens_total)}</td>
                    <td className="py-1 text-right font-mono text-muted-foreground">
                      {p.tokens_today > 0 ? formatTokens(p.tokens_today) : "—"}
                    </td>
                    <td className="py-1 text-right font-mono">{formatUsd(p.cost_total_usd)}</td>
                    <td className="py-1 text-right text-muted-foreground text-xs">
                      {p.last_activity_ms ? relativeTime(new Date(p.last_activity_ms).toISOString()) : "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section>
        <div className="flex items-baseline gap-2 mb-2">
          <h2 className="text-xs uppercase tracking-wider font-medium text-foreground">
            Pipelines ativas
          </h2>
          <span className="text-[13px] text-muted-foreground/50 font-mono">
            {loading ? "…" : activePipelines.length}
          </span>
        </div>
        {!loading && activePipelines.length === 0 ? (
          <p className="text-[13px] text-muted-foreground py-2">Sem pipelines ativas.</p>
        ) : (
          <ul className="flex flex-col gap-0.5 text-sm">
            {activePipelines.map((row: ActivePipelineRow) => {
              const variant = specVariant(row.spec.phase, row.spec.status);
              return (
                <li
                  key={`${row.projectId}/${row.spec.name}`}
                  className="flex items-center gap-2 px-2 py-1 rounded hover:bg-muted/40 cursor-pointer"
                  onClick={() =>
                    navigate(
                      `/project/${row.projectId}/spec/${encodeURIComponent(row.spec.name)}`,
                    )
                  }
                >
                  <StatusDot variant={variant} pulse={variant === "active"} />
                  <span className="text-muted-foreground text-[13px]">{row.projectName}</span>
                  <span className="text-muted-foreground/50">/</span>
                  <span className="font-mono">{row.spec.name}</span>
                  {row.spec.phase && (
                    <Badge variant="secondary" className="text-[11px] py-0">
                      {row.spec.phase}
                    </Badge>
                  )}
                  <span className="ml-auto text-muted-foreground text-[13px]">
                    {row.spec.started_at ? relativeTime(row.spec.started_at) : "—"}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <Separator />

      <section>
        <div className="flex items-baseline gap-2 mb-2">
          <h2 className="text-xs uppercase tracking-wider font-medium text-foreground">
            Atividade recente
          </h2>
          <span className="text-[13px] text-muted-foreground/50 font-mono">
            {loading ? "…" : timeline.length}
          </span>
        </div>
        {!loading && timeline.length === 0 ? (
          <p className="text-[13px] text-muted-foreground py-2">Sem eventos recentes.</p>
        ) : (
          <ul className="flex flex-col gap-0.5 text-sm">
            {timeline.map((row: TimelineRow, i: number) => (
              <li
                key={i}
                className="flex items-baseline gap-2 px-2 py-1 rounded hover:bg-muted/40"
              >
                <Badge variant="secondary" className="text-[11px] py-0 font-mono">
                  {row.event.event_type}
                </Badge>
                <span className="text-muted-foreground text-[13px] flex items-center gap-1">
                  <FolderGit2 className="h-3 w-3" />
                  {row.projectName}
                </span>
                {row.event.ts && (
                  <span className="text-muted-foreground text-[13px]">
                    {relativeTime(row.event.ts)}
                  </span>
                )}
                {row.event.summary && (
                  <span className="text-muted-foreground text-[13px]">
                    — {truncate(row.event.summary, 120)}
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
