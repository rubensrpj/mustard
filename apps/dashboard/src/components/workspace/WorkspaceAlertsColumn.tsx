import { useMemo, useState } from "react";
import { AlertTriangle } from "lucide-react";
import { cn } from "@/lib/utils";
import { relativeTime } from "@/lib/time";
import type { WorkspaceAlert } from "@/lib/types/specs";

interface WorkspaceAlertsColumnProps {
  alerts: WorkspaceAlert[];
  onAlertClick?: (alert: WorkspaceAlert) => void;
  className?: string;
}

const KIND_LABEL: Record<string, string> = {
  wave_failed: "Wave falhou",
  qa_fail: "QA falhou",
  build_broken: "Build quebrado",
  review_rejected: "Review rejeitado",
};

const COLLAPSE_THRESHOLD = 3;

function alertAccent(kind: string): string {
  switch (kind) {
    case "build_broken":
    case "wave_failed":
      return "text-[--color-error]";
    case "qa_fail":
      return "text-[--color-accent-mustard]";
    default:
      return "text-[--color-error]";
  }
}

/**
 * Group alerts by spec, preserving the original ordering of the first
 * occurrence of each spec. Returns an array of `[spec, alerts]` tuples so the
 * caller can render groups in a stable order without re-sorting on every
 * render.
 */
function groupBySpec(
  alerts: WorkspaceAlert[],
): Array<[string, WorkspaceAlert[]]> {
  const groups = new Map<string, WorkspaceAlert[]>();
  for (const alert of alerts) {
    const key = alert.spec;
    const bucket = groups.get(key);
    if (bucket) {
      bucket.push(alert);
    } else {
      groups.set(key, [alert]);
    }
  }
  return Array.from(groups.entries());
}

function AlertRow({
  alert,
  onAlertClick,
}: {
  alert: WorkspaceAlert;
  onAlertClick?: (a: WorkspaceAlert) => void;
}) {
  return (
    <li>
      <button
        type="button"
        onClick={() => onAlertClick?.(alert)}
        className={cn(
          "w-full text-left flex items-center gap-1.5 rounded px-2 py-1",
          "hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2",
          "focus-visible:ring-[--color-accent-mustard] transition-colors",
          "cursor-pointer",
        )}
        aria-label={`${KIND_LABEL[alert.kind] ?? alert.kind} em ${alert.spec}. Clique para detalhes.`}
      >
        <AlertTriangle
          className={cn("h-3 w-3 shrink-0", alertAccent(alert.kind))}
          aria-hidden
        />
        <span className="text-[11px] font-medium text-muted-foreground shrink-0">
          {KIND_LABEL[alert.kind] ?? alert.kind}
        </span>
        {alert.wave != null && (
          <span
            className="text-[11px] text-muted-foreground/60 tabular-nums shrink-0"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            onda {alert.wave}
          </span>
        )}
        {alert.ts && (
          <span className="text-[11px] text-muted-foreground/50 ml-auto shrink-0">
            {relativeTime(alert.ts)}
          </span>
        )}
      </button>
    </li>
  );
}

function AlertGroup({
  spec,
  alerts,
  onAlertClick,
}: {
  spec: string;
  alerts: WorkspaceAlert[];
  onAlertClick?: (a: WorkspaceAlert) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const collapsed = alerts.length > COLLAPSE_THRESHOLD && !expanded;
  const visible = collapsed ? alerts.slice(0, COLLAPSE_THRESHOLD) : alerts;

  return (
    <div className="flex flex-col gap-0.5 rounded border border-border/40 bg-card/30 p-1.5">
      <div className="flex items-center gap-1.5 px-1">
        <span
          className="font-mono text-[12px] truncate text-foreground/80 flex-1 min-w-0"
          title={spec}
        >
          {spec}
        </span>
        <span
          className="text-[11px] text-muted-foreground tabular-nums shrink-0"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {alerts.length}
        </span>
      </div>
      <ul className="flex flex-col gap-0.5">
        {visible.map((alert, i) => (
          <AlertRow
            key={`${alert.kind}-${alert.wave ?? "x"}-${alert.ts ?? i}`}
            alert={alert}
            onAlertClick={onAlertClick}
          />
        ))}
      </ul>
      {alerts.length > COLLAPSE_THRESHOLD && (
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className={cn(
            "text-[11px] text-[--color-accent-mustard] hover:underline",
            "self-start px-1 py-0.5 rounded focus-visible:outline-none",
            "focus-visible:ring-2 focus-visible:ring-[--color-accent-mustard]",
          )}
          aria-expanded={expanded}
        >
          {expanded
            ? "ocultar"
            : `ver todos (${alerts.length - COLLAPSE_THRESHOLD} restantes)`}
        </button>
      )}
    </div>
  );
}

export function WorkspaceAlertsColumn({
  alerts,
  onAlertClick,
  className,
}: WorkspaceAlertsColumnProps) {
  const grouped = useMemo(() => groupBySpec(alerts), [alerts]);

  if (alerts.length === 0) {
    return (
      <aside
        aria-label="Alertas do workspace"
        className={cn(
          "w-[280px] shrink-0 flex flex-col gap-2 rounded-lg border border-border bg-card/20 p-3",
          className,
        )}
      >
        <p className="text-[11px] uppercase tracking-wide text-muted-foreground font-medium mb-1">
          Alertas
        </p>
        <p className="text-[12px] text-muted-foreground/60">Nenhum problema detectado.</p>
      </aside>
    );
  }

  return (
    <aside
      aria-label="Alertas do workspace"
      className={cn(
        "w-[280px] shrink-0 flex flex-col gap-2 rounded-lg border border-border bg-card/20 p-3",
        className,
      )}
    >
      <p className="text-[11px] uppercase tracking-wide text-muted-foreground font-medium mb-1">
        Alertas{" "}
        <span
          className="text-[--color-error] tabular-nums"
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {alerts.length}
        </span>
      </p>

      <div className="flex flex-col gap-2">
        {grouped.map(([spec, specAlerts]) => (
          <AlertGroup
            key={spec}
            spec={spec}
            alerts={specAlerts}
            onAlertClick={onAlertClick}
          />
        ))}
      </div>
    </aside>
  );
}
