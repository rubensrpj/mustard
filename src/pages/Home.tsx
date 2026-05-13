import { Link, useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { FolderGit2 } from "lucide-react";
import { StatusDot } from "@/components/StatusDot";
import { useStore } from "@/lib/store";
import { discoverProjects } from "@/api/discovery";
import { relativeTime } from "@/lib/time";
import { AggregateOverview } from "@/components/AggregateOverview";
import { Separator } from "@/components/ui/separator";

export function Home() {
  const navigate = useNavigate();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const setSelectedProjectId = useStore((s) => s.setSelectedProjectId);

  const { data: projects, isFetching: discovering } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  if (!projectsRoot) {
    return (
      <p className="text-sm text-muted-foreground">
        Configure o diretório de projetos em{" "}
        <Link to="/settings" className="underline">Settings</Link>.
      </p>
    );
  }

  if (discovering && !projects) {
    return <p className="text-sm text-muted-foreground">Descobrindo projetos…</p>;
  }

  if (!projects || projects.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        Nenhum projeto encontrado em{" "}
        <code className="font-mono text-foreground">{projectsRoot}</code>.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-8">
      <AggregateOverview projects={projects} />

      <Separator />

      <section>
        <div className="flex items-baseline gap-2 mb-2">
          <h2 className="text-[11px] uppercase tracking-wider font-medium text-muted-foreground">
            Projetos
          </h2>
          <span className="text-[11px] text-muted-foreground/50 font-mono">{projects.length}</span>
        </div>
        <ul className="flex flex-col gap-0.5 text-sm">
          {projects.map((p) => (
            <li
              key={p.id}
              className="flex items-center gap-2 px-2 py-1 rounded hover:bg-muted/40 cursor-pointer"
              onClick={() => {
                setSelectedProjectId(p.id);
                navigate(`/project/${p.id}`);
              }}
            >
              <FolderGit2 className="h-3.5 w-3.5 text-muted-foreground" />
              <StatusDot
                variant={
                  p.last_activity_ms && Date.now() - p.last_activity_ms < 3_600_000
                    ? 'active'
                    : 'idle'
                }
                pulse={false}
                size="md"
              />
              <span>{p.name}</span>
              <span className="text-muted-foreground text-xs ml-auto">
                {p.last_activity_ms
                  ? relativeTime(new Date(p.last_activity_ms).toISOString())
                  : '—'}
              </span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
