import { cn } from "@/lib/utils";
import type { AcceptanceCriterion } from "@/lib/types/telemetry";

export interface CriteriaPanelProps {
  criteria: AcceptanceCriterion[];
  className?: string;
}

function relativeTime(ts: string | null): string {
  if (!ts) return "";
  const ms = Date.now() - Date.parse(ts);
  if (!Number.isFinite(ms) || ms < 0) return "";
  const m = Math.floor(ms / 60_000);
  if (m < 1) return "agora";
  if (m < 60) return `há ${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `há ${h}h`;
  return `há ${Math.floor(h / 24)}d`;
}

// Build a 30-slot sparkline from criteria (pass=1, fail=0)
function buildSparkline(criteria: AcceptanceCriterion[]): number[] {
  const slots = Array<number>(30).fill(0);
  // We don't have timestamps per slot, so approximate: most-recent-first
  const sorted = [...criteria].sort((a, b) => {
    const at = a.last_run_at ? Date.parse(a.last_run_at) : 0;
    const bt = b.last_run_at ? Date.parse(b.last_run_at) : 0;
    return bt - at;
  });
  sorted.slice(0, 30).forEach((c, i) => {
    slots[29 - i] = c.status === "pass" ? 1 : 0;
  });
  return slots;
}

function MiniSparkline({ data }: { data: number[] }) {
  const h = 20;
  const w = 60;
  const step = w / Math.max(data.length - 1, 1);

  const points = data
    .map((v, i) => `${i * step},${h - v * h * 0.8 - 2}`)
    .join(" ");

  return (
    <svg width={w} height={h} aria-hidden="true" className="shrink-0">
      <polyline
        points={points}
        fill="none"
        stroke="var(--primary, #e6c84a)"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
        opacity={0.8}
      />
    </svg>
  );
}

function shortSpec(name: string): string {
  return name.replace(/^\d{4}-\d{2}-\d{2}-/, "");
}

export function CriteriaPanel({ criteria, className }: CriteriaPanelProps) {
  if (criteria.length === 0) {
    return (
      <div className={cn("text-[12px] text-muted-foreground/60 py-2", className)}>
        Sem critérios executados no período
      </div>
    );
  }

  const passed = criteria.filter((c) => c.status === "pass").length;
  const total = criteria.length;
  const passPct = total > 0 ? Math.round((passed / total) * 100) : 0;

  const sparkline = buildSparkline(criteria);
  const failures = criteria
    .filter((c) => c.status !== "pass")
    .sort((a, b) => {
      const at = a.last_run_at ? Date.parse(a.last_run_at) : 0;
      const bt = b.last_run_at ? Date.parse(b.last_run_at) : 0;
      return bt - at;
    })
    .slice(0, 5);

  return (
    <div className={cn("grid grid-cols-[3fr_2fr] gap-4", className)}>
      {/* left — approval rate hero */}
      <div className="flex flex-col gap-2">
        <div
          className="text-4xl font-bold leading-none tabular-nums"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          <span
            className={
              passPct >= 80
                ? "text-[--intent-success]"
                : passPct >= 50
                  ? "text-[--primary]"
                  : "text-[--intent-error]"
            }
          >
            {passPct}%
          </span>
        </div>
        <p className="text-[11px] text-muted-foreground leading-none">
          taxa de aprovação
        </p>
        <MiniSparkline data={sparkline} />
        <p className="text-[10px] text-muted-foreground/60">
          {passed}/{total} no período
        </p>
      </div>

      {/* right — recent failures */}
      <div className="flex flex-col gap-1">
        <p className="text-[10px] text-muted-foreground mb-1">últimas falhas</p>
        {failures.length === 0 ? (
          <p className="text-[11px] text-[--intent-success]">Sem falhas recentes</p>
        ) : (
          failures.map((c) => (
            <div
              key={`${c.spec}-${c.id}`}
              className="text-[11px] text-muted-foreground truncate"
              title={`${c.id} · ${c.spec}`}
            >
              <span className="text-[--intent-error] font-medium">{c.id}</span>
              {" · "}
              <span className="text-foreground/70">{shortSpec(c.spec)}</span>
              {c.last_run_at && (
                <span className="text-muted-foreground/50">
                  {" · "}
                  {relativeTime(c.last_run_at)}
                </span>
              )}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
