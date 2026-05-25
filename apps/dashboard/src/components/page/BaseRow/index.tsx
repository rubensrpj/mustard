// Atom for any "list of things" UI: icon · label/summary · tokens · status · chevron.
// W6 (trace viewer) and W7 (Economia page) both render dense lists; this row keeps
// them visually coherent without each page redefining its own card chrome.

import type { ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { formatTokens } from "@/lib/types/economy";
import { StatPill } from "../StatPill";

export type RowStatus =
  | "draft"
  | "implementing"
  | "awaiting-qa"
  | "completed"
  | "archived";

export interface BaseRowProps {
  icon?: ReactNode;
  label: string;
  summary?: string;
  tokens?: number;
  status?: RowStatus;
  chevron?: boolean;
  onClick?: () => void;
  className?: string;
}

const DOT: Record<RowStatus, string> = {
  // TF remap: --ds-status-* → Binance intent/phase tokens (no status-dot tier in Binance pack)
  draft:         "bg-[--muted-foreground]",          /* TF remap: --ds-status-draft → --muted-foreground; inactive/pending */
  implementing:  "bg-[--color-phase-execute]",        /* TF remap: --ds-status-implementing → --color-phase-execute; in-flight */
  "awaiting-qa": "bg-[--color-phase-qa]",             /* TF remap: --ds-status-awaiting-qa → --color-phase-qa; QA lifecycle phase */
  completed:     "bg-[--intent-success]",             /* TF remap: --ds-status-completed → --intent-success; #0ecb81 Binance green */
  archived:      "bg-[--muted-foreground]",           /* TF remap: --ds-status-archived → --muted-foreground; inactive */
};

export function BaseRow({
  icon,
  label,
  summary,
  tokens,
  status,
  chevron = false,
  onClick,
  className,
}: BaseRowProps) {
  const interactive = typeof onClick === "function";
  return (
    <div
      role={interactive ? "button" : undefined}
      tabIndex={interactive ? 0 : undefined}
      onClick={onClick}
      onKeyDown={
        interactive
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick?.();
              }
            }
          : undefined
      }
      className={cn(
        // TF remap: --ds-radius-md → var(--radius-card) (8px card radius Binance)
        // TF remap: --ds-surface-base → --background (canvas)
        "flex items-center gap-3 px-3 py-2 rounded-[--radius-card]",
        "bg-[--background]",
        // TF remap: --ds-surface-hover → --accent; --ds-accent-primary → --primary
        interactive && "cursor-pointer hover:bg-[--accent] focus:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]/60",
        className,
      )}
    >
      {/* TF remap: --ds-text-tertiary → --muted-foreground; no tertiary tier in Binance */}
      {icon ? <span className="shrink-0 text-[--muted-foreground]">{icon}</span> : null}
      {status ? (
        <span
          aria-label={`status: ${status}`}
          className={cn("shrink-0 inline-block size-2 rounded-full", DOT[status])}
        />
      ) : null}
      <div className="min-w-0 flex-1">
        {/* TF remap: --ds-text-primary → --foreground; --ds-text-secondary → --muted-foreground */}
        <div className="text-[13px] font-medium text-[--foreground] truncate">{label}</div>
        {summary ? (
          <div className="text-[12px] text-[--muted-foreground] truncate">{summary}</div>
        ) : null}
      </div>
      {typeof tokens === "number" ? (
        <StatPill value={formatTokens(tokens)} unit="tok" />
      ) : null}
      {/* TF remap: --ds-text-tertiary → --muted-foreground */}
      {chevron ? (
        <ChevronRight size={14} className="shrink-0 text-[--muted-foreground]" />
      ) : null}
    </div>
  );
}
