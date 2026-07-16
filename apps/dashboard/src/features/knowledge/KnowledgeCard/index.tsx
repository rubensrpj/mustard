import { Markdown } from "@/components/page/Markdown";
import { relativeTime } from "@/lib/time";
import type { KnowledgeRow } from "@/lib/dashboard";

/**
 * One decision/lesson card. `title` is the captured headline (decision title /
 * lesson takeaway), `body` the optional context (rationale / trigger), `spec`
 * the pipeline attribution from the event envelope. Events carry no
 * confidence score, so none is rendered.
 */
export function KnowledgeCard({ row }: { row: KnowledgeRow }) {
  return (
    <div className="border border-border rounded p-3 flex flex-col gap-1.5">
      <div className="flex items-baseline gap-2">
        <h3 className="font-mono font-medium text-sm">{row.title}</h3>
        {row.ts && (
          <span className="ml-auto shrink-0 text-[11px] text-muted-foreground/60">
            {relativeTime(row.ts)}
          </span>
        )}
      </div>
      {row.spec && (
        <div className="text-[12px] text-muted-foreground">
          spec <span className="font-mono text-foreground">{row.spec}</span>
        </div>
      )}
      {row.body && (
        <div className="text-[13px] text-muted-foreground">
          <Markdown content={row.body} />
        </div>
      )}
    </div>
  );
}