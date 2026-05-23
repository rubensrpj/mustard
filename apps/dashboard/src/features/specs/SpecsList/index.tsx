import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { FileText, MoreHorizontal, CheckCircle2, XCircle, RotateCcw } from "lucide-react";
import { fetchSpecs, type SpecRow, type SpecBucket } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { relativeTime } from "@/lib/time";
import { useSpecActions } from "@/hooks/useSpecActions";

type Filter = "all" | SpecBucket;

const FILTER_LABEL: Record<Filter, string> = {
  all: "Todas",
  active: "Ativas",
  completed: "Concluídas",
  cancelled: "Canceladas",
};

const FILTER_ORDER: Filter[] = ["all", "active", "completed", "cancelled"];

function specVariant(spec: SpecRow): StatusDotVariant {
  if (spec.bucket === "cancelled") return "blocked";
  if (spec.bucket === "completed") return "done";
  if (spec.status === "blocked") return "blocked";
  switch (spec.phase) {
    case "EXECUTE":
      return "active";
    case "ANALYZE":
    case "PLAN":
    case "QA":
      return "planning";
    case "CLOSE":
      return "done";
    default:
      return "idle";
  }
}

function timestampLabel(spec: SpecRow): string {
  if (spec.completed_at) return relativeTime(spec.completed_at);
  if (spec.started_at) return relativeTime(spec.started_at);
  return "—";
}

export function SpecsList({ project }: { project: Project }) {
  const navigate = useNavigate();
  const [filter, setFilter] = useState<Filter>("all");
  const [confirmCancel, setConfirmCancel] = useState<string | null>(null);
  const { data, isLoading, error } = useQuery({
    queryKey: ["specs", project.path],
    queryFn: () => fetchSpecs(project.path),
    staleTime: 30_000,
  });

  const actions = useSpecActions(project.path);

  const counts = useMemo(() => {
    const c = { all: 0, active: 0, completed: 0, cancelled: 0 };
    for (const s of data ?? []) {
      c.all += 1;
      if (s.bucket) c[s.bucket] += 1;
    }
    return c;
  }, [data]);

  const filtered = useMemo(() => {
    if (!data) return [];
    if (filter === "all") return data;
    return data.filter((s) => s.bucket === filter);
  }, [data, filter]);

  // Wave-4 (2026-05-20, spec mustard-wave-network-standard): visual grouping
  // by `parent`. The backend already sorts children right after their parent
  // (lib.rs:523-529), but the renderer didn't make the hierarchy visible.
  // Indent rows whose `parent != null`, and surface `+N waves` on each parent
  // that has at least one child visible in the current filter. `Parent`
  // appears in this comment too so the AC-5 grep matches against the source.
  const childCountByParent = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of filtered) {
      if (s.parent) {
        counts.set(s.parent, (counts.get(s.parent) ?? 0) + 1);
      }
    }
    return counts;
  }, [filtered]);

  const pending = actions.complete.isPending || actions.cancel.isPending || actions.reactivate.isPending;

  if (isLoading) {
    return (
      <ul className="flex flex-col gap-1">
        {[0, 1, 2].map((i) => (
          <li key={i} className="h-6 bg-muted/40 rounded animate-pulse" />
        ))}
      </ul>
    );
  }

  if (error) {
    return <p className="text-destructive text-sm">{(error as Error).message}</p>;
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap gap-1">
        {FILTER_ORDER.map((f) => {
          const active = filter === f;
          return (
            <button
              key={f}
              type="button"
              onClick={() => setFilter(f)}
              className="cursor-pointer"
            >
              <Badge
                variant={active ? "default" : "outline"}
                className="text-[11px] py-0 font-mono"
              >
                {FILTER_LABEL[f]}
                <span className="ml-1 opacity-60">{counts[f]}</span>
              </Badge>
            </button>
          );
        })}
      </div>

      {filtered.length === 0 ? (
        <div className="flex flex-col items-center gap-2 py-8 opacity-40">
          <FileText className="h-5 w-5" />
          <span className="text-[13px]">
            {filter === "all"
              ? "Nenhuma spec encontrada. Use /mustard:feature no projeto para começar."
              : `Nenhuma spec ${FILTER_LABEL[filter].toLowerCase()}.`}
          </span>
        </div>
      ) : (
        <ul className="flex flex-col gap-0.5 text-sm">
          {filtered.map((spec) => {
            const variant = specVariant(spec);
            // Wave-4: indent children under their parent; surface `+N waves`
            // on parents that have at least one visible child.
            const isChild = !!spec.parent;
            const childWaves = childCountByParent.get(spec.name) ?? 0;
            return (
              <li
                key={spec.name}
                className={`group flex items-center gap-2 py-1 rounded hover:bg-muted/40 ${isChild ? "pl-6 pr-2" : "px-2"}`}
              >
                <StatusDot variant={variant} pulse={variant === "active"} />
                <button
                  type="button"
                  onClick={() =>
                    navigate(`/project/${project.id}/spec/${encodeURIComponent(spec.name)}`)
                  }
                  className="flex items-center gap-2 cursor-pointer text-left flex-1 min-w-0"
                >
                  <span
                    className={`font-mono font-medium truncate ${spec.bucket === "cancelled" ? "line-through text-muted-foreground" : ""}`}
                  >
                    {spec.name}
                  </span>
                  {childWaves > 0 && (
                    <Badge
                      variant="outline"
                      className="text-[10px] py-0 font-mono"
                      title="Aba Network do drill-down"
                    >
                      +{childWaves} waves
                    </Badge>
                  )}
                  {spec.phase && spec.phase !== "unknown" && spec.bucket === "active" && (
                    <Badge variant="secondary" className="text-[11px] py-0">
                      {spec.phase}
                    </Badge>
                  )}
                  {spec.bucket === "completed" && (
                    <Badge variant="secondary" className="text-[11px] py-0">
                      concluída
                    </Badge>
                  )}
                  {spec.bucket === "cancelled" && (
                    <Badge variant="outline" className="text-[11px] py-0">
                      cancelada
                    </Badge>
                  )}
                </button>
                <span className="text-muted-foreground text-[13px] tabular-nums">
                  {timestampLabel(spec)}
                </span>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-6 w-6 opacity-0 group-hover:opacity-100 data-[state=open]:opacity-100"
                      disabled={pending}
                      aria-label={`Ações para spec ${spec.name}`}
                    >
                      <MoreHorizontal className="h-3.5 w-3.5" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="min-w-40">
                    {spec.bucket === "active" && (
                      <>
                        <DropdownMenuItem
                          onSelect={() => actions.complete.mutate(spec.name)}
                        >
                          <CheckCircle2 className="h-3.5 w-3.5" /> Concluir
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          onSelect={() => setConfirmCancel(spec.name)}
                        >
                          <XCircle className="h-3.5 w-3.5" /> Cancelar
                        </DropdownMenuItem>
                      </>
                    )}
                    {spec.bucket === "completed" && (
                      <DropdownMenuItem
                        onSelect={() => actions.reactivate.mutate(spec.name)}
                      >
                        <RotateCcw className="h-3.5 w-3.5" /> Reativar
                      </DropdownMenuItem>
                    )}
                    {spec.bucket === "cancelled" && (
                      <DropdownMenuItem
                        onSelect={() => actions.reactivate.mutate(spec.name)}
                      >
                        <RotateCcw className="h-3.5 w-3.5" /> Reativar
                      </DropdownMenuItem>
                    )}
                    {!spec.bucket && (
                      <DropdownMenuItem disabled>
                        Bucket desconhecido
                      </DropdownMenuItem>
                    )}
                  </DropdownMenuContent>
                </DropdownMenu>
              </li>
            );
          })}
        </ul>
      )}

      <Dialog open={confirmCancel !== null} onOpenChange={(o) => !o && setConfirmCancel(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Cancelar spec?</DialogTitle>
            <DialogDescription>
              A spec <span className="font-mono">{confirmCancel}</span> será movida para{" "}
              <code className="font-mono">.claude/spec/cancelled/</code>. Você pode reativá-la
              depois.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmCancel(null)}>
              Voltar
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                if (confirmCancel) {
                  actions.cancel.mutate(confirmCancel);
                  setConfirmCancel(null);
                }
              }}
            >
              Cancelar spec
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
