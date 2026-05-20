import { cn } from "@/lib/utils";
import type { RtkBlock, HookFireCount, RoutingBlock, MeasuredBlock } from "@/lib/dashboard";
import type { PhaseSummary } from "@/lib/types/telemetry";
import type { PromptEconomy } from "@/api/promptEconomy";

// Inline sparkline (30 points → 60×20px SVG)
function MiniSparkline({ data, className }: { data: number[]; className?: string }) {
  if (data.length < 2) return null;
  const h = 20;
  const w = 60;
  const max = Math.max(...data, 1);
  const step = w / (data.length - 1);
  const pts = data.map((v, i) => `${i * step},${h - (v / max) * (h - 4) - 2}`).join(" ");

  return (
    <svg
      width={w}
      height={h}
      className={cn("shrink-0", className)}
      aria-hidden="true"
    >
      <polyline
        points={pts}
        fill="none"
        stroke="var(--color-accent-mustard, #e6c84a)"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
        opacity={0.85}
      />
    </svg>
  );
}

function formatTokens(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`;
  return String(n);
}

function formatPct(n: number | null | undefined): string {
  if (n == null) return "—";
  return `${n.toFixed(1)}%`;
}

function formatBytes(b: number): string {
  if (b >= 1_048_576) return `${(b / 1_048_576).toFixed(1)} MB`;
  if (b >= 1_024) return `${(b / 1_024).toFixed(0)} KB`;
  return `${b} B`;
}

function formatUsd(usd: number | null | undefined): string {
  if (usd == null) return "—";
  if (usd < 0.01) return "<$0.01";
  return `$${usd.toFixed(2)}`;
}

// ── RTK hero card ──────────────────────────────────────────────────────────────

function RtkCard({ rtk }: { rtk: RtkBlock }) {
  const daily = rtk.daily ?? [];
  const sparklineData = daily.map((d) => d.saved_tokens);

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border bg-card/40 p-4">
      <div className="flex items-start justify-between gap-2">
        <div>
          <p className="text-[11px] text-muted-foreground mb-1">tokens economizados · RTK</p>
          <div
            className="text-3xl font-bold tabular-nums leading-none text-[--color-accent-mustard]"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {rtk.available ? formatTokens(rtk.tokens_saved) : "—"}
          </div>
          <p className="text-[11px] text-muted-foreground mt-1">
            {rtk.savings_pct != null ? `${formatPct(rtk.savings_pct)} de redução` : "RTK indisponível"}
          </p>
        </div>
        <MiniSparkline data={sparklineData} />
      </div>

      {rtk.available && (
        <div className="grid grid-cols-2 gap-x-4 gap-y-1 pt-2 border-t border-border">
          <Stat label="comandos" value={String(rtk.total_commands ?? "—")} />
          <Stat label="tempo total" value={rtk.total_exec_time_ms != null ? `${Math.round(rtk.total_exec_time_ms / 1000)}s` : "—"} />
          <Stat label="entrada" value={formatTokens(rtk.input_tokens)} />
          <Stat label="saída" value={formatTokens(rtk.output_tokens)} />
        </div>
      )}
    </div>
  );
}

// ── Hook economy card ──────────────────────────────────────────────────────────

function HooksCard({ prevention }: { prevention: HookFireCount[] }) {
  const totalSaved = prevention.reduce((s, h) => s + h.tokens_saved, 0);

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border bg-card/40 p-4">
      <p className="text-[11px] text-muted-foreground">tokens economizados · hooks</p>
      <div
        className="text-2xl font-bold tabular-nums leading-none text-foreground"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {formatTokens(totalSaved)}
      </div>
      <div className="flex flex-col gap-0.5 mt-1">
        {prevention.slice(0, 4).map((h) => (
          <div key={h.hook} className="flex items-center justify-between gap-2 min-w-0">
            <span className="text-[10px] text-muted-foreground truncate">{h.hook}</span>
            <span
              className="text-[10px] text-muted-foreground tabular-nums"
              style={{ fontVariantNumeric: "tabular-nums" }}
            >
              {h.fires}
            </span>
          </div>
        ))}
        {prevention.length === 0 && (
          <p className="text-[10px] text-muted-foreground/50">sem dados</p>
        )}
      </div>
    </div>
  );
}

// ── Routing card ───────────────────────────────────────────────────────────────

function RoutingCard({ routing }: { routing: RoutingBlock }) {
  const total = routing.blocks + routing.allows;
  const blockPct = total > 0 ? Math.round((routing.blocks / total) * 100) : 0;

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border bg-card/40 p-4">
      <p className="text-[11px] text-muted-foreground">roteamento de modelo</p>
      <div
        className="text-2xl font-bold tabular-nums leading-none text-foreground"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {routing.blocks}
        <span className="text-[13px] font-normal text-muted-foreground ml-1">bloqueios</span>
      </div>
      <p className="text-[10px] text-muted-foreground">
        {blockPct}% de {total} decisões
      </p>
    </div>
  );
}

// ── Prompt Economy trio ────────────────────────────────────────────────────────

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-[10px] text-muted-foreground leading-none">{label}</p>
      <p
        className="text-[13px] font-medium tabular-nums leading-snug"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {value}
      </p>
    </div>
  );
}

function PromptEconomyBlock({
  measured,
  phases,
  promptEconomy,
}: {
  measured: MeasuredBlock;
  phases: PhaseSummary[];
  promptEconomy?: PromptEconomy;
}) {
  const sub = promptEconomy?.subtractions;
  const cost = promptEconomy?.cost;
  const claudeEvents = promptEconomy?.claude_events;

  // Context avoidance ratio
  const avoidanceRatio =
    sub && sub.context_sent_bytes + sub.context_avoided_bytes > 0
      ? (sub.context_avoided_bytes / (sub.context_sent_bytes + sub.context_avoided_bytes)) * 100
      : null;

  return (
    <div className="grid grid-cols-3 gap-3">
      {/* Cache — hero: USD measured */}
      <div className="rounded-lg border border-border bg-card/40 p-3 col-span-1">
        <p className="text-[10px] text-muted-foreground mb-1">custo medido (Anthropic)</p>
        <div
          className="text-xl font-bold tabular-nums text-foreground"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {cost != null ? formatUsd(cost.usd_total) : formatTokens(measured.tokens_total)}
        </div>
        <p className="text-[10px] text-muted-foreground mt-0.5">
          {cost != null
            ? `hoje: ${formatUsd(cost.by_session.length > 0 ? cost.usd_total : null)}`
            : `hoje: ${formatTokens(measured.tokens_today)}`}
        </p>
        {claudeEvents && (
          <p className="text-[10px] text-muted-foreground/70 mt-0.5">
            {claudeEvents.session_count} sessões ·{" "}
            {Math.round(claudeEvents.active_time_seconds / 60)} min ativos
          </p>
        )}
      </div>

      {/* Contexto — context sent vs avoided */}
      <div className="rounded-lg border border-border bg-card/40 p-3">
        <p className="text-[10px] text-muted-foreground mb-1">contexto evitado</p>
        <div
          className="text-xl font-bold tabular-nums text-foreground"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {sub != null ? formatBytes(sub.context_avoided_bytes) : "—"}
        </div>
        <p className="text-[10px] text-muted-foreground mt-0.5">
          {avoidanceRatio != null
            ? `${avoidanceRatio.toFixed(0)}% do total`
            : `${phases.length} fases ativas`}
        </p>
        {sub != null && (
          <p className="text-[10px] text-muted-foreground/70 mt-0.5">
            enviado: {formatBytes(sub.context_sent_bytes)}
          </p>
        )}
      </div>

      {/* Eventos — sessions sparkline or phases fallback */}
      <div className="rounded-lg border border-border bg-card/40 p-3">
        <p className="text-[10px] text-muted-foreground mb-1">
          {claudeEvents != null ? "eventos · sessão" : "eventos · fases"}
        </p>
        <div
          className="text-xl font-bold tabular-nums text-foreground"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {claudeEvents != null
            ? claudeEvents.session_count
            : phases.reduce((s, p) => s + p.events_count, 0) || "—"}
        </div>
        <p className="text-[10px] text-muted-foreground mt-0.5">
          {sub != null
            ? `${sub.event_count} subtrações`
            : `${phases.length} fases ativas`}
        </p>
        <MiniSparkline
          data={phases[0]?.sparkline ?? []}
          className="mt-1"
        />
      </div>
    </div>
  );
}

// ── Main export ────────────────────────────────────────────────────────────────

export interface EconomySectionProps {
  rtk: RtkBlock;
  measured: MeasuredBlock;
  prevention: HookFireCount[];
  routing: RoutingBlock;
  phases: PhaseSummary[];
  /** Optional rich prompt-economy payload (USD cost, context subtractions, sessions). */
  promptEconomy?: PromptEconomy;
  className?: string;
}

export function EconomySection({
  rtk,
  measured,
  prevention,
  routing,
  phases,
  promptEconomy,
  className,
}: EconomySectionProps) {
  // Total hero: RTK + hooks combined
  const totalSaved =
    (rtk.tokens_saved ?? 0) +
    prevention.reduce((s, h) => s + h.tokens_saved, 0);
  const totalSparkline = (rtk.daily ?? []).map((d) => d.saved_tokens);

  const canaryTail = promptEconomy?.freshness?.canary_tail ?? null;

  return (
    <div className={cn("flex flex-col gap-6 animate-mount-fade", className)}>
      {/* Hero number */}
      <div className="flex flex-col gap-1">
        <p className="text-[12px] text-muted-foreground">tokens economizados no total</p>
        <div className="flex items-end gap-3">
          <span
            className="font-bold tabular-nums text-[--color-accent-mustard] leading-none"
            style={{
              fontSize: "clamp(48px, 7vw, 72px)",
              fontVariantNumeric: "tabular-nums",
            }}
          >
            {formatTokens(totalSaved)}
          </span>
          <MiniSparkline data={totalSparkline} className="mb-2" />
        </div>
      </div>

      {/* 3 cards: RTK (2fr) + hooks (1fr) + routing (1fr) */}
      <div className="grid gap-3" style={{ gridTemplateColumns: "2fr 1fr 1fr" }}>
        <RtkCard rtk={rtk} />
        <HooksCard prevention={prevention} />
        <RoutingCard routing={routing} />
      </div>

      {/* Prompt Economy section */}
      <div className="flex flex-col gap-2">
        <h2 className="text-[14px] font-semibold tracking-tight text-foreground">
          Economia de prompts
        </h2>
        <PromptEconomyBlock measured={measured} phases={phases} promptEconomy={promptEconomy} />
      </div>

      {/* Diagnostics canary tail */}
      <details className="border border-border rounded-lg overflow-hidden">
        <summary className="cursor-pointer px-4 py-2 text-[11px] text-muted-foreground hover:text-foreground transition-colors select-none">
          Diagnostics (canary tail)
        </summary>
        <div className="px-4 pb-4 pt-2 flex flex-col gap-3">
          {/* OTEL canary tail lines from promptEconomy.freshness */}
          {canaryTail && canaryTail.length > 0 && (
            <div>
              <p className="text-[10px] text-muted-foreground mb-1">OTEL canary tail</p>
              <div className="flex flex-col gap-0.5">
                {canaryTail.map((line, i) => (
                  <p key={i} className="text-[11px] font-mono text-muted-foreground/70 break-all">
                    {line}
                  </p>
                ))}
              </div>
            </div>
          )}

          <div>
            <p className="text-[10px] text-muted-foreground mb-1">hooks prevention (raw)</p>
            <div className="flex flex-col gap-0.5">
              {prevention.length === 0 && (
                <p className="text-[11px] text-muted-foreground/50">sem dados</p>
              )}
              {prevention.map((h) => (
                <div key={h.hook} className="grid grid-cols-[1fr_auto_auto_auto] gap-3 text-[11px]">
                  <span className="text-muted-foreground truncate">{h.hook}</span>
                  <span
                    className="tabular-nums text-muted-foreground"
                    style={{ fontVariantNumeric: "tabular-nums" }}
                  >
                    {h.fires} fires
                  </span>
                  <span
                    className="tabular-nums text-muted-foreground"
                    style={{ fontVariantNumeric: "tabular-nums" }}
                  >
                    {formatTokens(h.tokens_saved)} saved
                  </span>
                  <span className="text-muted-foreground/50">
                    session: {h.session_fires}
                  </span>
                </div>
              ))}
            </div>
          </div>

          <div>
            <p className="text-[10px] text-muted-foreground mb-1">routing by note</p>
            <div className="flex flex-col gap-0.5">
              {routing.by_note.length === 0 && (
                <p className="text-[11px] text-muted-foreground/50">sem dados</p>
              )}
              {routing.by_note.map((n) => (
                <div key={n.note} className="flex items-center justify-between gap-2 text-[11px]">
                  <span className="text-muted-foreground">{n.note}</span>
                  <span
                    className="tabular-nums text-muted-foreground"
                    style={{ fontVariantNumeric: "tabular-nums" }}
                  >
                    {n.count}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* Context budget breakdown from promptEconomy.subtractions */}
          {promptEconomy?.subtractions && (
            <div>
              <p className="text-[10px] text-muted-foreground mb-1">context budget · by_wave</p>
              <div className="flex flex-col gap-0.5">
                {promptEconomy.subtractions.by_wave.slice(0, 10).map((w) => (
                  <div key={w.wave} className="grid grid-cols-[auto_1fr_auto_auto] gap-2 text-[11px]">
                    <span className="font-mono text-muted-foreground/60">w{w.wave}</span>
                    <span className="text-muted-foreground/50">{w.count}x</span>
                    <span className="tabular-nums text-muted-foreground" style={{ fontVariantNumeric: "tabular-nums" }}>
                      {formatBytes(w.sent_bytes)} enviado
                    </span>
                    <span className="tabular-nums text-muted-foreground/60" style={{ fontVariantNumeric: "tabular-nums" }}>
                      -{formatBytes(w.avoided_bytes)}
                    </span>
                  </div>
                ))}
                {promptEconomy.subtractions.by_wave.length === 0 && (
                  <p className="text-[11px] text-muted-foreground/50">sem dados</p>
                )}
              </div>
            </div>
          )}
        </div>
      </details>
    </div>
  );
}
