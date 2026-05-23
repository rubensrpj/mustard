import { Badge } from "@/components/ui/badge";
import { Markdown } from "@/components/page/Markdown";
import type { KnowledgeRow } from "@/lib/dashboard";

function formatConfidence(c: number): string {
  return `${Math.round(c * 100)}%`;
}

function confBadgeClass(c: number): string {
  if (c > 0.8) return "border-[--intent-success]/50 text-[--intent-success]";
  if (c >= 0.5) return "border-[--primary]/50 text-[--primary]";
  return "border-border text-muted-foreground";
}

// "high-hook-retry-2026-04-09-foo.metrics" → "2026-04-09-foo"
function stripPrefix(name: string, prefix: string): string {
  const rest = name.startsWith(prefix) ? name.slice(prefix.length) : name;
  return rest.replace(/\.metrics$/, "");
}

// Pulls trailing `Tool breakdown: { ... }` out of a description string.
// Returns { lead, breakdown } where breakdown is the parsed object or null.
function parseToolBreakdown(description: string): {
  lead: string;
  breakdown: Record<string, number> | null;
} {
  const m = description.match(/(.*?)Tool breakdown:\s*(\{[^}]*\})\s*$/s);
  if (!m) return { lead: description, breakdown: null };
  try {
    const parsed = JSON.parse(m[2]);
    if (parsed && typeof parsed === "object") {
      const clean: Record<string, number> = {};
      for (const [k, v] of Object.entries(parsed)) {
        if (typeof v === "number") clean[k] = v;
      }
      return { lead: m[1].trim().replace(/\.$/, ""), breakdown: clean };
    }
  } catch {
    // fallthrough to raw
  }
  return { lead: description, breakdown: null };
}

function ToolBreakdownChips({ breakdown }: { breakdown: Record<string, number> }) {
  const entries = Object.entries(breakdown).sort((a, b) => b[1] - a[1]);
  return (
    <div className="flex flex-wrap gap-1">
      {entries.map(([tool, count]) => (
        <span
          key={tool}
          className="inline-flex items-center gap-1 rounded border border-border bg-muted/30 px-1.5 py-0 text-[11px] font-mono"
        >
          <span className="text-foreground">{tool}</span>
          <span className="text-muted-foreground">×{count}</span>
        </span>
      ))}
    </div>
  );
}

function HighHookRetryBody({ row }: { row: KnowledgeRow }) {
  const spec = stripPrefix(row.name, "high-hook-retry-");
  const { lead, breakdown } = parseToolBreakdown(row.description);
  const retries = lead.match(/(\d+)\s+hook-level\s+retries/i)?.[1] ?? null;
  return (
    <div className="flex flex-col gap-2 text-[13px]">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 text-muted-foreground">
        <span>
          spec <span className="font-mono text-foreground">{spec}</span>
        </span>
        {retries && (
          <span>
            retries <span className="font-mono text-foreground">{retries}</span>
          </span>
        )}
      </div>
      {breakdown && <ToolBreakdownChips breakdown={breakdown} />}
      {!breakdown && row.description && (
        <p className="text-muted-foreground whitespace-pre-wrap">{row.description}</p>
      )}
    </div>
  );
}

function HeavyPipelineBody({ row }: { row: KnowledgeRow }) {
  const spec = stripPrefix(row.name, "heavy-pipeline-");
  const apiCalls = row.description.match(/(\d+)\s+API\s+calls/i)?.[1] ?? null;
  return (
    <div className="flex flex-col gap-2 text-[13px]">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 text-muted-foreground">
        <span>
          spec <span className="font-mono text-foreground">{spec}</span>
        </span>
        {apiCalls && (
          <span>
            api calls <span className="font-mono text-foreground">{apiCalls}</span>
          </span>
        )}
      </div>
      <Badge variant="outline" className="self-start text-[11px]">
        Consider splitting into smaller scope
      </Badge>
    </div>
  );
}

function GenericBody({ description }: { description: string }) {
  return (
    <div className="text-[13px] text-muted-foreground">
      <Markdown content={description} />
    </div>
  );
}

function pickBody(row: KnowledgeRow) {
  if (row.name.startsWith("high-hook-retry-")) return <HighHookRetryBody row={row} />;
  if (row.name.startsWith("heavy-pipeline-")) return <HeavyPipelineBody row={row} />;
  return <GenericBody description={row.description} />;
}

export function KnowledgeCard({ row }: { row: KnowledgeRow }) {
  return (
    <div className="border border-border rounded p-3 flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <h3 className="font-mono font-medium text-sm">{row.name}</h3>
        <span
          className={`ml-auto inline-flex items-center rounded-full px-1.5 py-0 text-[11px] border ${confBadgeClass(row.confidence)}`}
        >
          {formatConfidence(row.confidence)}
        </span>
      </div>
      {row.description && pickBody(row)}
    </div>
  );
}
