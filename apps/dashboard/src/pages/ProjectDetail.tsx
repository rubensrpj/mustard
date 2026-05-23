import { useEffect, useMemo } from "react";
import { Link, useParams, useSearchParams } from "react-router";
import { Layers, ChefHat, BookOpen, Activity, type LucideIcon } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useProject } from "@/hooks/useProject";
import { useStore } from "@/lib/store";
import { queryClient } from "@/lib/query-client";
import type { Project } from "@/api/discovery";
import { StatusDot, type StatusDotVariant } from "@/components/page/StatusDot";
import { relativeTime } from "@/lib/time";
import { Separator } from "@/components/ui/separator";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { SpecsList } from "@/features/specs/SpecsList";
import type { SkillMeta } from "@/lib/dashboard";
import { fetchActivePipelines } from "@/lib/dashboard";
import { LivePipelineCard } from "@/features/workspace/LivePipelineCard";

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

function eventVariant(eventType: string): StatusDotVariant {
  switch (eventType) {
    case "tool.use":
    case "commit-gate.check":
      return "idle";
    case "pipeline.phase":
      return "planning";
    case "qa.result":
      return "done";
    case "agent.start":
    case "session.start":
      return "active";
    default:
      return "idle";
  }
}

function SectionHeading({
  title,
  count,
  loading,
  icon: Icon,
}: {
  title: string;
  count: number;
  loading: boolean;
  icon?: LucideIcon;
}) {
  return (
    <div className="flex items-baseline gap-2 mb-2">
      {Icon && <Icon className="h-4 w-4 text-foreground self-center" />}
      <h2
        className={`text-xs uppercase tracking-wider font-medium ${Icon ? "text-foreground" : "text-muted-foreground"}`}
      >
        {title}
      </h2>
      <span className="text-[13px] text-muted-foreground/50 font-mono">
        {loading ? "…" : count}
      </span>
    </div>
  );
}

function EmptyBlock({ icon: Icon, text }: { icon: LucideIcon; text: string }) {
  return (
    <div className="flex flex-col items-center gap-2 py-3 opacity-40">
      <Icon className="h-5 w-5" />
      <span className="text-[13px]">{text}</span>
    </div>
  );
}

export function ProjectDetail() {
  const { id } = useParams<{ id: string }>();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const setSelectedProjectId = useStore((s) => s.setSelectedProjectId);
  const projects = queryClient.getQueryData<Project[]>(['discover', projectsRoot]);
  const project = projects?.find((p) => p.id === id) ?? null;

  const [searchParams, setSearchParams] = useSearchParams();
  const tab = searchParams.get("tab") === "about" ? "about" : "specs";

  useEffect(() => {
    if (id) setSelectedProjectId(id);
  }, [id, setSelectedProjectId]);

  const { subprojects, recipes, skills, recentEvents, loading, error } = useProject(project);

  // Wave 3 (2026-05-22): watcher-driven via "pipeline-state" — no poll needed.
  const { data: activePipelines } = useQuery({
    queryKey: ['active-pipelines', project?.path],
    queryFn: () => fetchActivePipelines(project!.path),
    enabled: !!project,
    staleTime: 5_000,
  });

  const groupedSkills = useMemo(() => {
    const groups: Record<string, SkillMeta[]> = {};
    for (const s of skills ?? []) {
      const key = s.source ?? "unknown";
      (groups[key] = groups[key] ?? []).push(s);
    }
    return groups;
  }, [skills]);

  function sourceLabel(src: string): string {
    if (src === "foundation") return "Foundation";
    if (src === "command") return "Commands";
    if (src.startsWith("subproject:")) return `Subprojeto: ${src.slice("subproject:".length)}`;
    return src;
  }

  const sourceOrder = ["foundation", "command"];
  const orderedSkillKeys = Object.keys(groupedSkills).sort((a, b) => {
    const ai = sourceOrder.indexOf(a);
    const bi = sourceOrder.indexOf(b);
    if (ai !== -1 && bi !== -1) return ai - bi;
    if (ai !== -1) return -1;
    if (bi !== -1) return 1;
    return a.localeCompare(b);
  });

  if (!project) {
    return (
      <div className="text-sm text-muted-foreground">
        Projeto não encontrado. Volte ao{" "}
        <Link to="/" className="underline">Home</Link> ou configure root em{" "}
        <Link to="/settings" className="underline">Settings</Link>.
      </div>
    );
  }

  if (error) {
    return <p className="text-destructive text-sm">{error}</p>;
  }

  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-col gap-1 mb-4">
        <nav className="text-[13px] text-muted-foreground">
          Mustard / Projetos / <span className="text-foreground">{project.name}</span>
        </nav>
        <h1 className="text-base font-medium">{project.name}</h1>
      </div>

      {activePipelines && activePipelines.length > 0 && (
        <section className="mb-4">
          <h2 className="text-xs uppercase tracking-wider font-medium text-muted-foreground mb-1">
            Em execução
          </h2>
          <ul className="flex flex-col gap-0.5">
            {activePipelines.slice(0, 3).map((pipeline) => (
              <LivePipelineCard key={pipeline.spec_name} pipeline={pipeline} />
            ))}
          </ul>
        </section>
      )}

      <Tabs
        value={tab}
        onValueChange={(v) => {
          const next = new URLSearchParams(searchParams);
          next.set("tab", v);
          setSearchParams(next, { replace: true });
        }}
      >
        <TabsList variant="line" className="gap-4 h-9 text-sm mb-4">
          <TabsTrigger value="specs" className="text-sm px-1">
            Specs
          </TabsTrigger>
          <TabsTrigger value="about" className="text-sm px-1">
            About
          </TabsTrigger>
        </TabsList>

        <TabsContent value="specs">
          <SpecsList project={project} />
        </TabsContent>

        <TabsContent value="about">
          <section>
            <SectionHeading
              title="Subprojects"
              count={subprojects?.length ?? 0}
              loading={loading}
              icon={Layers}
            />
            {loading ? (
              <p className="text-muted-foreground text-sm">Carregando…</p>
            ) : subprojects && subprojects.length === 0 ? (
              <EmptyBlock icon={Layers} text="Nenhum subprojeto detectado." />
            ) : (
              <ul className="flex flex-col gap-0.5 text-sm">
                {subprojects?.map((s) => (
                  <li
                    key={s.name}
                    className="flex items-center gap-2 px-2 py-1 rounded hover:bg-muted/40"
                  >
                    <StatusDot variant="idle" />
                    <span>{s.name}</span>
                    {s.role && (
                      <Badge variant="outline" className="text-[11px] py-0">
                        {s.role}
                      </Badge>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </section>

          <Separator className="my-4" />

          <section>
            <SectionHeading
              title="Recipes"
              count={recipes?.length ?? 0}
              loading={loading}
              icon={ChefHat}
            />
            {loading ? (
              <p className="text-muted-foreground text-sm">Carregando…</p>
            ) : (
              <ul className="flex flex-col gap-0.5 text-sm">
                {recipes?.map((r) => (
                  <li
                    key={r.name}
                    className="flex items-baseline gap-2 px-2 py-1 rounded hover:bg-muted/40"
                  >
                    <span className="font-medium">{r.name}</span>
                    <span className="text-muted-foreground text-[13px]">
                      — {truncate(r.description, 120)}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <Separator className="my-4" />

          <section>
            <SectionHeading
              title="Skills"
              count={skills?.length ?? 0}
              loading={loading}
              icon={BookOpen}
            />
            {loading ? (
              <p className="text-muted-foreground text-sm">Carregando…</p>
            ) : (
              <div className="flex flex-col gap-3">
                {orderedSkillKeys.map((key) => (
                  <div key={key} className="flex flex-col gap-1">
                    <h3 className="text-xs uppercase tracking-wider font-medium text-muted-foreground">
                      {sourceLabel(key)}
                    </h3>
                    <ul className="flex flex-col gap-0.5">
                      {groupedSkills[key].map((s) => (
                        <li key={s.name} className="text-[13px]">
                          <span className="font-mono">{s.name}</span>
                          {s.description && (
                            <span className="text-muted-foreground"> — {s.description}</span>
                          )}
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </div>
            )}
          </section>

          <Separator className="my-4" />

          <section>
            <SectionHeading
              title="Eventos recentes"
              count={recentEvents?.length ?? 0}
              loading={loading}
              icon={Activity}
            />
            {loading ? (
              <p className="text-muted-foreground text-sm">Carregando…</p>
            ) : (
              <ScrollArea className="max-h-[400px] pr-2">
                <ul className="flex flex-col gap-0.5 text-sm">
                  {recentEvents?.map((e, i) => {
                    const variant = eventVariant(e.event_type);
                    return (
                      <li
                        key={i}
                        className="flex items-baseline gap-2 px-2 py-1 rounded hover:bg-muted/40"
                      >
                        <StatusDot
                          variant={variant}
                          pulse={variant === "active"}
                          className="self-center"
                        />
                        <Badge variant="secondary" className="text-[11px] py-0 font-mono">
                          {e.event_type}
                        </Badge>
                        {e.tool_name && (
                          <Badge variant="outline" className="text-[10px] py-0 font-mono">
                            {e.tool_name}
                          </Badge>
                        )}
                        {e.target && (
                          <code
                            className="text-xs text-muted-foreground font-mono truncate max-w-xl"
                            title={e.target}
                          >
                            {e.target}
                          </code>
                        )}
                        {e.spec && (
                          <span
                            className="text-xs text-muted-foreground font-mono"
                            title={e.spec}
                          >
                            {e.spec.replace(/^\d{4}-\d{2}-\d{2}-/, "")}
                          </span>
                        )}
                        {e.wave != null && (
                          <span className="text-xs text-muted-foreground">W{e.wave}</span>
                        )}
                        {!e.tool_name && !e.target && !e.spec && e.wave == null && e.summary && (
                          <span className="text-muted-foreground text-[13px]">— {truncate(e.summary, 200)}</span>
                        )}
                        {e.ts && (
                          <span className="text-muted-foreground text-[13px] ml-auto">{relativeTime(e.ts)}</span>
                        )}
                      </li>
                    );
                  })}
                </ul>
              </ScrollArea>
            )}
          </section>
        </TabsContent>
      </Tabs>
    </div>
  );
}
