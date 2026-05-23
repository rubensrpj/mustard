import { cn } from "@/lib/utils";
import { midTruncate } from "@/lib/text";
import type { FileCount } from "@/lib/types/specs";

interface WorkspaceEffortFooterProps {
  topFiles: FileCount[];
  className?: string;
}

export function WorkspaceEffortFooter({ topFiles, className }: WorkspaceEffortFooterProps) {
  // Show top 5 only
  const files = topFiles.slice(0, 5);
  const maxCount = files[0]?.count ?? 1;

  if (files.length === 0) {
    return null;
  }

  return (
    <footer
      aria-label="Arquivos mais tocados hoje"
      className={cn(
        "flex flex-col gap-2 border-t border-border/40 pt-3",
        className,
      )}
    >
      <p className="text-[10px] text-muted-foreground/70 uppercase tracking-wide">
        Arquivos mais tocados hoje
      </p>
      <ul className="flex flex-col gap-1">
        {files.map((f) => {
          const pct = maxCount > 0 ? (f.count / maxCount) * 100 : 0;
          // AC-13: middle-truncation — never truncate at the start
          const display = midTruncate(f.path, 20, 12);

          return (
            <li key={f.path} className="flex items-center gap-2 min-w-0">
              <span
                className="text-[11px] text-muted-foreground flex-1 min-w-0 overflow-hidden whitespace-nowrap"
                title={f.path}
              >
                {display}
              </span>
              <div className="flex-shrink-0 w-16 h-1 rounded-full bg-muted overflow-hidden">
                <div
                  className="h-full rounded-full bg-[--primary]/60"
                  style={{ width: `${pct.toFixed(1)}%` }}
                  aria-hidden
                />
              </div>
              <span
                className="text-[11px] text-muted-foreground w-6 text-right tabular-nums shrink-0"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                {f.count}
              </span>
            </li>
          );
        })}
      </ul>
    </footer>
  );
}
