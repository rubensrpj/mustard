import { useMemo, useState } from "react";
import { AlertTriangle, Ban, XCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";
import { useTranslate } from "@/lib/i18n";
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
  blocked: "Bloqueado",
};

const COLLAPSE_THRESHOLD = 3;

/**
 * Mapping of alert kind → (Lucide icon, badge variant). Wave 8 (2026-05-21,
 * spec `2026-05-20-economia-moat-unification/wave-8-visao-geral-revamp`) — the
 * column used to render plain text; this version threads a semantic icon and a
 * coloured `<Badge>` so severity reads at a glance from the new 50/50 split
 * layout in `Workspace.tsx`.
 */
const KIND_ICON: Record<string, typeof AlertTriangle> = {
  wave_failed: XCircle,
  qa_fail: AlertTriangle,
  build_broken: XCircle,
  review_rejected: XCircle,
  blocked: Ban,
};

const KIND_BADGE: Record<string, "error" | "warning" | "info"> = {
  wave_failed: "error",
  qa_fail: "warning",
  build_broken: "error",
  review_rejected: "error",
  blocked: "info",
};

function alertAccent(kind: string): string {
  switch (kind) {
    case "build_broken":
    case "wave_failed":
    case "review_rejected":
      return "text-[--ds-intent-error]";
    case "qa_fail":
      return "text-[--ds-intent-warning]";
    case "blocked":
      return "text-[--ds-intent-info]";
    default:
      return "text-[--ds-intent-error]";
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
  const Icon = KIND_ICON[alert.kind] ?? AlertTriangle;
  const badgeVariant = KIND_BADGE[alert.kind] ?? "error";
  return (
    <li>
      <button
        type="button"
        onClick={() => onAlertClick?.(alert)}
        className={cn(
          "w-full text-left flex items-center gap-1.5 rounded px-2 py-1",
          "hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2",
          "focus-visible:ring-[--ds-accent-primary]/60 transition-colors",
          "cursor-pointer",
        )}
        aria-label={`${KIND_LABEL[alert.kind] ?? alert.kind} em ${alert.spec}. Clique para detalhes.`}
      >
        <Icon
          className={cn("h-3.5 w-3.5 shrink-0", alertAccent(alert.kind))}
          aria-hidden
        />
        <Badge variant={badgeVariant} className="shrink-0">
          {KIND_LABEL[alert.kind] ?? alert.kind}
        </Badge>
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
            "text-[11px] text-[--ds-accent-primary] hover:underline",
            "self-start px-1 py-0.5 rounded focus-visible:outline-none",
            "focus-visible:ring-2 focus-visible:ring-[--ds-accent-primary]/60",
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

/**
 * Wave 8 update: dropped the fixed `w-[280px]` so the column can sit inside a
 * 50/50 grid alongside `<WorkspaceFilesRanking>` (see `Workspace.tsx`). The
 * width is now driven by the parent grid cell.
 */
export function WorkspaceAlertsColumn({
  alerts,
  onAlertClick,
  className,
}: WorkspaceAlertsColumnProps) {
  const t = useTranslate();
  const grouped = useMemo(() => groupBySpec(alerts), [alerts]);

  if (alerts.length === 0) {
    return (
      <aside
        aria-label={t("workspace.alerts")}
        className={cn(
          "flex flex-col gap-2 rounded-lg border border-border bg-card/20 p-3",
          className,
        )}
      >
        <p className="text-[11px] uppercase tracking-wide text-muted-foreground font-medium mb-1">
          {t("workspace.alerts")}
        </p>
        <p className="text-[12px] text-muted-foreground/60">Nenhum problema detectado.</p>
      </aside>
    );
  }

  return (
    <aside
      aria-label={t("workspace.alerts")}
      className={cn(
        "flex flex-col gap-2 rounded-lg border border-border bg-card/20 p-3",
        className,
      )}
    >
      <p className="text-[11px] uppercase tracking-wide text-muted-foreground font-medium mb-1">
        {t("workspace.alerts")}{" "}
        <span
          className="text-[--ds-intent-error] tabular-nums"
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
