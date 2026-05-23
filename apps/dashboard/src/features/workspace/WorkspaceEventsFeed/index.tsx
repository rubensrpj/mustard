import { useNavigate } from "react-router";
import { cn } from "@/lib/utils";
import { DataCard, SectionHeader, EmptyState } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { useWorkspaceEventsFeed } from "@/hooks/useWorkspaceEventsFeed";
import type { FeedEvent } from "@/lib/dashboard";

interface WorkspaceEventsFeedProps {
  repoPath: string;
}

type BadgeVariant = "info" | "success" | "warning" | "error";

/**
 * Map an event `kind` (e.g. `pipeline.status`) to a semantic badge variant.
 * Defaults to `info` so unknown kinds still render coherently.
 */
function kindToVariant(kind: string): BadgeVariant {
  switch (kind) {
    case "pipeline.complete":
      return "success";
    case "pipeline.dispatch_failure":
      return "error";
    case "pipeline.status":
    case "pipeline.scope":
      return "info";
    default:
      return "info";
  }
}

/**
 * Lightweight relative-time formatter — avoids pulling a date library in.
 * Returns "há Xmin", "há Xh", "ontem HH:mm", otherwise a localized date.
 */
function relativeTime(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  const now = Date.now();
  const diffMs = now - t;
  const diffMin = Math.floor(diffMs / 60_000);
  if (diffMin < 1) return "agora";
  if (diffMin < 60) return `há ${diffMin}min`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `há ${diffHr}h`;
  const d = new Date(t);
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const sameDay = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate();
  const pad = (n: number) => (n < 10 ? `0${n}` : String(n));
  if (sameDay(d, yesterday)) {
    return `ontem ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }
  return d.toLocaleDateString("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function EventRow({
  event,
  onSpecClick,
}: {
  event: FeedEvent;
  onSpecClick: (spec: string) => void;
}) {
  const variant = kindToVariant(event.kind);
  return (
    <li className="flex items-start gap-2 px-2 py-1.5 border-b border-border/30 last:border-b-0">
      <Badge variant={variant} className="shrink-0 mt-0.5">
        {event.kind}
      </Badge>
      <span
        className="text-[11px] text-muted-foreground tabular-nums shrink-0 mt-1"
        style={{ fontVariantNumeric: "tabular-nums" }}
      >
        {relativeTime(event.ts)}
      </span>
      <span className="text-[12.5px] text-foreground/80 flex-1 min-w-0 truncate mt-0.5">
        {event.payload_summary}
      </span>
      {event.spec && (
        <a
          href={`/specs#${event.spec}`}
          onClick={(e) => {
            e.preventDefault();
            onSpecClick(event.spec as string);
          }}
          className={cn(
            "font-mono text-[11.5px] text-[--primary] hover:underline",
            "truncate max-w-[180px] shrink-0 mt-0.5",
            "focus-visible:outline-none focus-visible:ring-2",
            "focus-visible:ring-[--primary] rounded",
          )}
          title={event.spec}
        >
          {event.spec}
        </a>
      )}
    </li>
  );
}

/**
 * Live workspace events feed — newest first. Polled by the hook at 5s; the
 * scroll container caps height so the card fits the overview grid.
 */
export function WorkspaceEventsFeed({ repoPath }: WorkspaceEventsFeedProps) {
  const navigate = useNavigate();
  const { data, isLoading } = useWorkspaceEventsFeed(repoPath, 50);

  const events = data ?? [];

  return (
    <DataCard padded>
      <SectionHeader
        title="Feed de eventos"
        right={
          <span
            className="tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            {events.length}
          </span>
        }
      />

      {isLoading && events.length === 0 ? (
        <p className="mt-3 text-[12.5px] text-muted-foreground/70">Carregando…</p>
      ) : events.length === 0 ? (
        <EmptyState
          className="mt-3"
          title="Nenhum evento recente"
          description="Eventos do harness aparecem aqui em tempo real."
        />
      ) : (
        <div className="mt-3 max-h-[480px] overflow-y-auto rounded-md border border-border/40 bg-card/20">
          <ul className="flex flex-col">
            {events.map((event) => (
              <EventRow
                key={event.id}
                event={event}
                onSpecClick={(spec) => navigate(`/specs#${spec}`)}
              />
            ))}
          </ul>
        </div>
      )}
    </DataCard>
  );
}
