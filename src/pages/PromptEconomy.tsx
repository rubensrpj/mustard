import { useStore } from "@/lib/store";
import { useProjects } from "@/lib/dashboard";
import { usePromptEconomy, useCollectorHealth } from "@/hooks/usePromptEconomy";
import type { CollectorHealth } from "@/api/promptEconomy";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatUsd } from "@/lib/format";
import { cn } from "@/lib/utils";
import type { PromptEconomy } from "@/api/promptEconomy";

// ── Format helpers (Wave 5) ─────────────────────────────────────────────────
// Inlined here because the three concerns (bytes, seconds, USD) only meet on
// this page; promoting them to lib/format.ts would just add indirection.

function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  if (n < 1024) return `${Math.round(n)} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatActiveTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "0s";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    return `${m}m ${s}s`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

/**
 * The badge now comes straight from the unified `collector_health` Tauri
 * command (`CollectorHealth` = "live" | "stale" | "off"). There is no longer a
 * page-local `deriveBadge` — Telemetry and this page render the SAME state
 * from the SAME source. Mapping to colours:
 *  - live  → green  : metric within 5 min AND collector healthy
 *  - stale → amber  : real historical data, but old / collector down
 *  - off   → red    : OTEL never received a metric → genuinely not configured
 */
function StatusBadge({ state }: { state: CollectorHealth }) {
  const label =
    state === "live"
      ? "OTEL ativo"
      : state === "stale"
        ? "Coletor parado"
        : "OTEL não configurado";
  const dot =
    state === "live"
      ? "bg-emerald-500"
      : state === "stale"
        ? "bg-amber-500"
        : "bg-rose-500";
  return (
    <Badge variant="outline" className="gap-1.5 text-[11px] font-normal">
      <span className={cn("inline-block h-2 w-2 rounded-full", dot)} aria-hidden />
      {label}
    </Badge>
  );
}

function BlockHeader({
  title,
  hint,
}: {
  title: string;
  hint: string;
}) {
  return (
    <div className="flex items-baseline justify-between gap-3 mb-3">
      <h2 className="text-[13px] font-medium text-foreground tracking-tight">
        {title}
      </h2>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="text-[10px] uppercase tracking-wider text-muted-foreground hover:text-foreground transition-colors"
              aria-label={`Sobre: ${title}`}
            >
              info
            </button>
          </TooltipTrigger>
          <TooltipContent side="left" className="max-w-xs text-[11.5px] leading-relaxed">
            {hint}
          </TooltipContent>
        </Tooltip>
      </TooltipProvider>
    </div>
  );
}

function CostBlock({ cost }: { cost: PromptEconomy["cost"] }) {
  const topModels = cost.by_model.slice(0, 4);
  return (
    <Card size="sm" className="ring-foreground/5">
      <CardContent>
        <BlockHeader
          title="Cache da API"
          hint="Medido pela Anthropic API via OTEL nativo. Inclui desconto de cache automaticamente — o número já reflete o que você de fato paga."
        />
        <div className="flex flex-col gap-3">
          <div className="flex items-baseline gap-2">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              USD total
            </span>
            <span className="text-2xl font-mono tabular-nums text-indigo-400 leading-none">
              {formatUsd(cost.usd_total)}
            </span>
          </div>
          {topModels.length > 0 ? (
            <div className="flex flex-col gap-1.5 pt-1 border-t border-border/40">
              <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Por modelo
              </span>
              <ul className="flex flex-col gap-1">
                {topModels.map((m) => (
                  <li
                    key={m.model}
                    className="flex items-baseline justify-between gap-3 text-[12.5px]"
                  >
                    <span className="font-mono text-muted-foreground truncate">
                      {m.model}
                    </span>
                    <span className="font-mono tabular-nums text-foreground shrink-0">
                      {formatUsd(m.usd)}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <p className="text-[11.5px] text-muted-foreground italic">
              Sem dados por modelo ainda.
            </p>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

function SubtractionsBlock({
  subtractions,
}: {
  subtractions: PromptEconomy["subtractions"];
}) {
  const rows: { label: string; bytes: number; count: number }[] = [
    {
      label: "diff-vs-full",
      bytes: subtractions.diff_vs_full_bytes,
      count: subtractions.diff_vs_full_count,
    },
    {
      label: "wave-slice",
      bytes: subtractions.wave_slice_bytes,
      count: subtractions.wave_slice_count,
    },
    {
      label: "review-diff-first",
      bytes: subtractions.review_diff_first_bytes,
      count: subtractions.review_diff_first_count,
    },
    {
      label: "analyze-diff-skip",
      bytes: subtractions.analyze_diff_skip_bytes,
      count: subtractions.analyze_diff_skip_count,
    },
  ];
  const totalBytes = rows.reduce((a, r) => a + r.bytes, 0);
  return (
    <Card size="sm" className="ring-foreground/5">
      <CardContent>
        <BlockHeader
          title="Bytes omitidos pelo Mustard"
          hint="Bytes que o orquestrador escolheu não enviar — economia contrafactual. Não é dinheiro; é contexto que nunca virou prompt."
        />
        <div className="flex flex-col gap-3">
          <div className="flex items-baseline gap-2">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Total
            </span>
            <span className="text-2xl font-mono tabular-nums text-violet-400 leading-none">
              {formatBytes(totalBytes)}
            </span>
          </div>
          <ul className="flex flex-col gap-1.5 pt-1 border-t border-border/40">
            {rows.map((r) => (
              <li
                key={r.label}
                className="flex items-baseline justify-between gap-3 text-[12.5px]"
              >
                <span className="font-mono text-muted-foreground">{r.label}</span>
                <span className="flex items-baseline gap-2 shrink-0">
                  <span className="font-mono tabular-nums text-foreground">
                    {formatBytes(r.bytes)}
                  </span>
                  <span className="text-[11px] text-muted-foreground tabular-nums">
                    ({r.count} {r.count === 1 ? "evento" : "eventos"})
                  </span>
                </span>
              </li>
            ))}
          </ul>
        </div>
      </CardContent>
    </Card>
  );
}

function ClaudeEventsBlock({
  events,
}: {
  events: PromptEconomy["claude_events"];
}) {
  return (
    <Card size="sm" className="ring-foreground/5">
      <CardContent>
        <BlockHeader
          title="Eventos Claude Code"
          hint="Telemetria operacional do Claude Code: número de sessões iniciadas e tempo ativo total. Vem do mesmo stream OTEL, mas mede uso (não dinheiro)."
        />
        <div className="grid grid-cols-2 gap-4">
          <div className="flex flex-col gap-1">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Sessions
            </span>
            <span className="text-2xl font-mono tabular-nums text-foreground leading-none">
              {events.session_count.toLocaleString()}
            </span>
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Active time
            </span>
            <span className="text-2xl font-mono tabular-nums text-foreground leading-none">
              {formatActiveTime(events.active_time_seconds)}
            </span>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function CanaryTail({ lines }: { lines: string[] }) {
  return (
    <Card size="sm" className="ring-rose-500/20 border-rose-500/30">
      <CardContent>
        <div className="flex items-baseline gap-2 mb-2">
          <span className="text-[11px] uppercase tracking-[0.08em] font-medium text-rose-400">
            Canary tail
          </span>
          <span className="text-[11px] text-muted-foreground">
            últimas {lines.length} linhas
          </span>
        </div>
        <pre className="font-mono text-[11px] leading-relaxed text-muted-foreground/80 whitespace-pre-wrap break-all bg-background/40 rounded-md p-3 max-h-64 overflow-auto">
          {lines.join("\n")}
        </pre>
      </CardContent>
    </Card>
  );
}

export function PromptEconomy() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const projects = useProjects();
  const project = projects.find((p) => p.id === activeWorkspaceId) ?? null;
  const path = project?.path ?? null;

  const { data, isLoading, error } = usePromptEconomy(path);
  const { data: collectorHealth } = useCollectorHealth(path);

  if (!projectsRoot) {
    return (
      <Card size="sm" className="ring-foreground/5">
        <CardContent className="flex flex-col gap-1">
          <p className="text-sm font-medium">Configure o diretório de projetos</p>
          <p className="text-[13px] text-muted-foreground">
            Vá em Settings e aponte para a pasta onde estão seus repos.
          </p>
        </CardContent>
      </Card>
    );
  }
  if (!activeWorkspaceId || !project) {
    return (
      <Card size="sm" className="ring-foreground/5">
        <CardContent className="flex flex-col gap-1">
          <p className="text-sm font-medium">Selecione um workspace</p>
          <p className="text-[13px] text-muted-foreground">
            Use o seletor no topo da sidebar para escolher um projeto.
          </p>
        </CardContent>
      </Card>
    );
  }

  // Single source: the unified collector_health command. Defaults to "off"
  // until the query resolves — same state Telemetry shows from the same hook.
  const badge: CollectorHealth = collectorHealth ?? "off";
  const allZero =
    !!data &&
    data.cost.usd_total === 0 &&
    data.subtractions.wave_slice_bytes === 0 &&
    data.subtractions.diff_vs_full_bytes === 0 &&
    data.subtractions.review_diff_first_bytes === 0 &&
    data.subtractions.analyze_diff_skip_bytes === 0 &&
    data.claude_events.session_count === 0 &&
    data.claude_events.active_time_seconds === 0;

  return (
    <div className="flex flex-col gap-6 w-full">
      {/* Header */}
      <div className="flex flex-col gap-1.5">
        <nav className="text-[12px] text-muted-foreground flex items-center gap-1.5 flex-wrap">
          Mustard <span className="opacity-50">/</span>
          <span className="text-foreground whitespace-nowrap">Prompt Economy</span>
          <span className="opacity-50">/</span>
          <span className="font-mono">{project.name}</span>
        </nav>
        <div className="flex items-center justify-between gap-3 flex-wrap">
          <h1 className="text-xl font-medium tracking-tight">Prompt Economy</h1>
          <StatusBadge state={badge} />
        </div>
        <p className="text-[13px] text-muted-foreground leading-relaxed max-w-3xl">
          Três blocos honestos: o que a Anthropic API cobrou (medido), o que o
          Mustard escolheu não enviar (contrafactual) e a telemetria operacional
          do Claude Code. Cada bloco vem de uma fonte distinta — nada de números
          inventados.
        </p>
      </div>

      {/* Error state */}
      {error && (
        <Card size="sm" className="ring-foreground/5 border-destructive/40">
          <CardContent>
            <p className="text-[13px] text-destructive">
              Erro ao carregar prompt economy: {error.message}
            </p>
          </CardContent>
        </Card>
      )}

      {/* Loading skeleton */}
      {isLoading && !data && (
        <section className="grid grid-cols-1 lg:grid-cols-3 gap-3">
          {[0, 1, 2].map((i) => (
            <Card size="sm" key={i} className="ring-foreground/5">
              <CardContent>
                <div className="flex flex-col gap-3">
                  <div className="h-4 w-32 bg-muted/40 rounded animate-pulse" />
                  <div className="h-8 w-24 bg-muted/40 rounded animate-pulse" />
                  <div className="h-3 w-40 bg-muted/30 rounded animate-pulse" />
                </div>
              </CardContent>
            </Card>
          ))}
        </section>
      )}

      {/* Empty state — all three blocks zero */}
      {data && allZero && (
        <Card size="sm" className="ring-foreground/5">
          <CardContent className="flex flex-col items-center gap-2 py-8 text-center">
            <p className="text-[13.5px] font-medium text-foreground">
              Sem atividade
            </p>
            <p className="text-[12.5px] text-muted-foreground leading-relaxed max-w-md">
              Rode <code className="font-mono text-foreground">/mustard:feature</code>{" "}
              ou <code className="font-mono text-foreground">/mustard:bugfix</code>{" "}
              em algum projeto para começar a alimentar este painel.
            </p>
          </CardContent>
        </Card>
      )}

      {/* Three honest blocks */}
      {data && !allZero && (
        <section className="grid grid-cols-1 lg:grid-cols-3 gap-3">
          <CostBlock cost={data.cost} />
          <SubtractionsBlock subtractions={data.subtractions} />
          <ClaudeEventsBlock events={data.claude_events} />
        </section>
      )}

      {/* Canary tail — only when the collector is off (never saw a metric) */}
      {data && badge === "off" && data.freshness.canary_tail && data.freshness.canary_tail.length > 0 && (
        <CanaryTail lines={data.freshness.canary_tail} />
      )}
    </div>
  );
}
