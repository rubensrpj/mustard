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

export function WorkspaceAlertsColumn({
  alerts,
  onAlertClick,
  className,
}: WorkspaceAlertsColumnProps) {
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
        "w-[280px] shrink-0 flex flex-col gap-1.5 rounded-lg border border-border bg-card/20 p-3",
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

      <ul className="flex flex-col gap-1.5">
        {alerts.map((alert, i) => (
          <li key={`${alert.spec}-${alert.kind}-${i}`}>
            <button
              type="button"
              onClick={() => onAlertClick?.(alert)}
              className={cn(
                "w-full text-left flex flex-col gap-0.5 rounded p-2",
                "hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2",
                "focus-visible:ring-[--color-accent-mustard] transition-colors",
                "cursor-pointer",
              )}
              aria-label={`${KIND_LABEL[alert.kind] ?? alert.kind}: ${alert.spec}. Clique para detalhes.`}
            >
              <div className="flex items-center gap-1.5 min-w-0">
                <AlertTriangle
                  className={cn("h-3 w-3 shrink-0", alertAccent(alert.kind))}
                  aria-hidden
                />
                <span className="text-[11px] font-medium text-muted-foreground shrink-0">
                  {KIND_LABEL[alert.kind] ?? alert.kind}
                </span>
                {alert.wave != null && (
                  <span className="text-[11px] text-muted-foreground/60 tabular-nums shrink-0"
                    style={{ fontVariantNumeric: "tabular-nums" }}
                  >
                    onda {alert.wave}
                  </span>
                )}
              </div>
              <span
                className="font-mono text-[12px] truncate text-foreground/80"
                title={alert.spec}
              >
                {alert.spec}
              </span>
              {alert.ts && (
                <span className="text-[11px] text-muted-foreground/50">
                  {relativeTime(alert.ts)}
                </span>
              )}
            </button>
          </li>
        ))}
      </ul>
    </aside>
  );
}
