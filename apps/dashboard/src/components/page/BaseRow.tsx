// Atom for any "list of things" UI: icon · label/summary · tokens · status · chevron.
// W6 (trace viewer) and W7 (Economia page) both render dense lists; this row keeps
// them visually coherent without each page redefining its own card chrome.

import type { ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { formatTokens } from "@/lib/types/economy";
import { StatPill } from "./StatPill";

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
  draft:         "bg-[--ds-status-draft]",
  implementing:  "bg-[--ds-status-implementing]",
  "awaiting-qa": "bg-[--ds-status-awaiting-qa]",
  completed:     "bg-[--ds-status-completed]",
  archived:      "bg-[--ds-status-archived]",
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
        "flex items-center gap-3 px-3 py-2 rounded-[--ds-radius-md]",
        "bg-[--ds-surface-base]",
        interactive && "cursor-pointer hover:bg-[--ds-surface-hover] focus:outline-none focus-visible:ring-2 focus-visible:ring-[--ds-accent-primary]/60",
        className,
      )}
    >
      {icon ? <span className="shrink-0 text-[--ds-text-tertiary]">{icon}</span> : null}
      {status ? (
        <span
          aria-label={`status: ${status}`}
          className={cn("shrink-0 inline-block size-2 rounded-full", DOT[status])}
        />
      ) : null}
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-[--ds-text-primary] truncate">{label}</div>
        {summary ? (
          <div className="text-[12px] text-[--ds-text-secondary] truncate">{summary}</div>
        ) : null}
      </div>
      {typeof tokens === "number" ? (
        <StatPill value={formatTokens(tokens)} unit="tok" />
      ) : null}
      {chevron ? (
        <ChevronRight size={14} className="shrink-0 text-[--ds-text-tertiary]" />
      ) : null}
    </div>
  );
}
