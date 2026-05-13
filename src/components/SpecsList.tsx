import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { FileText } from "lucide-react";
import { fetchSpecs, type SpecRow } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";
import { StatusDot, type StatusDotVariant } from "@/components/StatusDot";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";

function specVariant(spec: SpecRow): StatusDotVariant {
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
  const { data, isLoading, error } = useQuery({
    queryKey: ["specs", project.path],
    queryFn: () => fetchSpecs(project.path),
    staleTime: 30_000,
  });

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

  if (!data || data.length === 0) {
    return (
      <div className="flex flex-col items-center gap-2 py-8 opacity-40">
        <FileText className="h-5 w-5" />
        <span className="text-xs">
          Nenhuma spec encontrada. Use /mustard:feature no projeto para começar.
        </span>
      </div>
    );
  }

  return (
    <ul className="flex flex-col gap-0.5 text-sm">
      {data.map((spec) => {
        const variant = specVariant(spec);
        return (
          <li
            key={spec.name}
            onClick={() =>
              navigate(`/project/${project.id}/spec/${encodeURIComponent(spec.name)}`)
            }
            className="flex items-center gap-2 px-2 py-1 rounded hover:bg-muted/40 cursor-pointer"
          >
            <StatusDot variant={variant} pulse={variant === "active"} />
            <span className="font-mono font-medium">{spec.name}</span>
            {spec.phase && (
              <Badge variant="secondary" className="text-[10px] py-0">
                {spec.phase}
              </Badge>
            )}
            {spec.status && (
              <Badge variant="outline" className="text-[10px] py-0">
                {spec.status}
              </Badge>
            )}
            <span className="ml-auto text-muted-foreground text-xs">
              {timestampLabel(spec)}
            </span>
          </li>
        );
      })}
    </ul>
  );
}
