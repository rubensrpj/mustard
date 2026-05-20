import { cn } from "@/lib/utils";

/**
 * Visual mini bar showing the pass/fail/skip split of QA Acceptance Criteria.
 * Shows a 14px-wide proportional bar + compact numeric breakdown next to it.
 *
 * When all three are zero, renders an em-dash instead.
 */
export interface AcBreakdownProps {
  pass: number;
  fail: number;
  skip: number;
  className?: string;
}

export function AcBreakdown({ pass, fail, skip, className }: AcBreakdownProps) {
  const total = pass + fail + skip;
  if (total === 0) {
    return <span className={cn("text-muted-foreground/50 text-[12px]", className)}>—</span>;
  }
  const passW = (pass / total) * 100;
  const failW = (fail / total) * 100;
  const skipW = (skip / total) * 100;
  return (
    <div
      className={cn("flex items-center gap-2 min-w-0", className)}
      title={`${pass} passou · ${fail} falhou · ${skip} pulou (total ${total})`}
    >
      <div className="flex h-1 w-14 rounded-full overflow-hidden bg-muted/30 shrink-0">
        {pass > 0 && <div className="bg-[--color-ok]/80" style={{ width: `${passW}%` }} />}
        {fail > 0 && <div className="bg-[--color-error]/80" style={{ width: `${failW}%` }} />}
        {skip > 0 && <div className="bg-[--color-accent-mustard]/80" style={{ width: `${skipW}%` }} />}
      </div>
      <span className="font-mono text-[11px] tabular-nums shrink-0">
        <span className="text-[--color-ok]">{pass}</span>
        {fail > 0 && <span className="text-muted-foreground/40">/</span>}
        {fail > 0 && <span className="text-[--color-error]">{fail}</span>}
        {skip > 0 && <span className="text-muted-foreground/40">/</span>}
        {skip > 0 && <span className="text-[--color-accent-mustard]">{skip}</span>}
        <span className="text-muted-foreground/40"> de {total}</span>
      </span>
    </div>
  );
}
