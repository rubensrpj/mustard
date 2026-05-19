import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router";
import { useEffect, useMemo } from "react";
import { useStore } from "@/lib/store";
import { queryClient } from "@/lib/query-client";
import { fetchSpecs, fetchSpecMarkdown } from "@/lib/dashboard";
import type { Project } from "@/api/discovery";
import { Badge } from "@/components/ui/badge";
import { relativeTime } from "@/lib/time";

function extractSection(md: string, heading: string): string | null {
  const re = new RegExp(`^##\\s+${heading.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*$`, "m");
  const m = md.match(re);
  if (!m || m.index === undefined) return null;
  const start = m.index + m[0].length;
  const tail = md.slice(start);
  const next = tail.match(/^##\s+/m);
  return (next && next.index !== undefined ? tail.slice(0, next.index) : tail).trim();
}

interface AcItem {
  id: string;
  text: string;
  command: string | null;
  checked: boolean;
}

function parseAcceptanceCriteria(section: string): AcItem[] {
  const items: AcItem[] = [];
  for (const line of section.split(/\r?\n/)) {
    const m = line.match(/^-\s*\[( |x)\]\s*(AC-\d+):\s*(.*)$/);
    if (!m) continue;
    const checked = m[1] === "x";
    const id = m[2];
    const rest = m[3];
    let text = rest;
    let command: string | null = null;
    const cmdIdx = rest.indexOf("— Command:");
    if (cmdIdx >= 0) {
      text = rest.slice(0, cmdIdx).trim();
      const after = rest.slice(cmdIdx + "— Command:".length).trim();
      const cm = after.match(/^`(.*)`\s*$/);
      command = cm ? cm[1] : after;
    }
    items.push({ id, text, command, checked });
  }
  return items;
}

interface ChecklistNode {
  type: "heading" | "checkbox" | "text";
  text: string;
  checked?: boolean;
}

function parseChecklist(section: string): ChecklistNode[] {
  const nodes: ChecklistNode[] = [];
  for (const raw of section.split(/\r?\n/)) {
    const line = raw.trimEnd();
    if (!line.trim()) continue;
    const h3 = line.match(/^###\s+(.+)$/);
    if (h3) {
      nodes.push({ type: "heading", text: h3[1] });
      continue;
    }
    const cb = line.match(/^\s*-\s*\[( |x)\]\s*(.*)$/);
    if (cb) {
      nodes.push({ type: "checkbox", checked: cb[1] === "x", text: cb[2] });
      continue;
    }
    nodes.push({ type: "text", text: line.replace(/^[-*]\s+/, "") });
  }
  return nodes;
}

export function SpecDetail() {
  const { id, specName: rawSpecName } = useParams<{ id: string; specName: string }>();
  const specName = rawSpecName ? decodeURIComponent(rawSpecName) : "";
  const projectsRoot = useStore((s) => s.projectsRoot);
  const setSelectedProjectId = useStore((s) => s.setSelectedProjectId);
  const projects = queryClient.getQueryData<Project[]>(["discover", projectsRoot]) ?? [];
  const project = projects.find((p) => p.id === id) ?? null;

  useEffect(() => {
    if (id) setSelectedProjectId(id);
  }, [id, setSelectedProjectId]);

  const { data: specs } = useQuery({
    queryKey: ["specs", project?.path],
    queryFn: () => fetchSpecs(project!.path),
    enabled: !!project,
    staleTime: 30_000,
  });
  const row = specs?.find((s) => s.name === specName) ?? null;

  const {
    data: markdown,
    isLoading: mdLoading,
    error: mdError,
  } = useQuery({
    queryKey: ["spec-markdown", project?.path, specName],
    queryFn: () => fetchSpecMarkdown(project!.path, specName),
    enabled: !!project && !!specName,
  });

  const acItems = useMemo(() => {
    if (!markdown) return [];
    const section =
      extractSection(markdown, "Critérios de Aceitação") ??
      extractSection(markdown, "Acceptance Criteria");
    return section ? parseAcceptanceCriteria(section) : [];
  }, [markdown]);

  const checklistNodes = useMemo(() => {
    if (!markdown) return [];
    const section =
      extractSection(markdown, "Checklist") ??
      extractSection(markdown, "Tarefas") ??
      extractSection(markdown, "Tasks");
    return section ? parseChecklist(section) : [];
  }, [markdown]);

  if (!project) {
    return (
      <div className="text-sm text-muted-foreground">
        Projeto não encontrado. Volte ao{" "}
        <Link to="/" className="underline">
          Home
        </Link>
        .
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-start justify-between gap-2 mb-4">
        <div className="flex flex-col gap-1 min-w-0">
          <nav className="text-xs text-muted-foreground">
            Mustard / Projetos /{" "}
            <Link to={`/project/${project.id}?tab=about`} className="hover:underline">
              {project.name}
            </Link>{" "}
            /{" "}
            <Link to={`/project/${project.id}?tab=specs`} className="hover:underline">
              Specs
            </Link>{" "}
            / <span className="text-foreground">{specName}</span>
          </nav>
          <h1 className="text-base font-medium font-mono break-all">{specName}</h1>
          <div className="flex items-center gap-2 flex-wrap">
            {row?.phase && (
              <Badge variant="secondary" className="text-[10px] py-0">
                {row.phase}
              </Badge>
            )}
            {row?.status && (
              <Badge variant="outline" className="text-[10px] py-0">
                {row.status}
              </Badge>
            )}
            {row?.started_at && (
              <span className="text-xs text-muted-foreground">
                started {relativeTime(row.started_at)}
              </span>
            )}
            {row?.completed_at && (
              <span className="text-xs text-muted-foreground">
                completed {relativeTime(row.completed_at)}
              </span>
            )}
          </div>
        </div>
        <Link
          to={`/project/${project.id}?tab=specs`}
          className="text-xs text-muted-foreground hover:text-foreground border border-border rounded px-2 py-1 shrink-0"
        >
          ← Voltar para Specs
        </Link>
      </div>

      {mdLoading && (
        <div className="flex flex-col gap-2">
          {[0, 1, 2].map((i) => (
            <div key={i} className="h-16 bg-muted/40 rounded animate-pulse" />
          ))}
        </div>
      )}

      {mdError && (
        <p className="text-destructive text-sm">{(mdError as Error).message}</p>
      )}

      {markdown && (
        <div className="flex flex-col gap-6">
          <section>
            <h2 className="text-[11px] uppercase tracking-wider font-medium text-foreground mb-2">
              Acceptance Criteria
            </h2>
            {acItems.length === 0 ? (
              <p className="text-xs text-muted-foreground">Sem AC definidos.</p>
            ) : (
              <ol className="flex flex-col gap-1.5 text-sm">
                {acItems.map((ac) => (
                  <li key={ac.id} className="flex items-baseline gap-2 flex-wrap">
                    <span className="font-mono text-xs text-muted-foreground">{ac.id}</span>
                    <span className={ac.checked ? "line-through text-muted-foreground" : ""}>
                      {ac.text}
                    </span>
                    {ac.command && (
                      <code className="font-mono text-xs bg-muted px-1 py-0.5 rounded break-all">
                        {ac.command}
                      </code>
                    )}
                  </li>
                ))}
              </ol>
            )}
          </section>

          <section>
            <h2 className="text-[11px] uppercase tracking-wider font-medium text-foreground mb-2">
              Checklist
            </h2>
            {checklistNodes.length === 0 ? (
              <p className="text-xs text-muted-foreground">Sem checklist.</p>
            ) : (
              <ul className="flex flex-col gap-0.5">
                {checklistNodes.map((node, i) => {
                  if (node.type === "heading") {
                    return (
                      <h3
                        key={i}
                        className="text-xs uppercase tracking-wider text-muted-foreground mt-3 mb-1"
                      >
                        {node.text}
                      </h3>
                    );
                  }
                  if (node.type === "checkbox") {
                    return (
                      <label key={i} className="flex items-baseline gap-2 text-xs">
                        <input
                          type="checkbox"
                          disabled
                          checked={node.checked}
                          readOnly
                          className="accent-foreground"
                        />
                        <span className={node.checked ? "line-through text-muted-foreground" : ""}>
                          {node.text}
                        </span>
                      </label>
                    );
                  }
                  return (
                    <p key={i} className="text-xs text-muted-foreground">
                      {node.text}
                    </p>
                  );
                })}
              </ul>
            )}
          </section>

          <section>
            <h2 className="text-[11px] uppercase tracking-wider font-medium text-foreground mb-2">
              Affected files
            </h2>
            {!row || row.affected_files.length === 0 ? (
              <p className="text-xs text-muted-foreground">Sem arquivos registrados.</p>
            ) : (
              <ul className="font-mono text-xs flex flex-col gap-0.5">
                {row.affected_files.map((f) => (
                  <li key={f} className="text-muted-foreground">
                    {f}
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
