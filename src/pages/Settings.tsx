import { useQuery } from '@tanstack/react-query';
import { open } from '@tauri-apps/plugin-dialog';
import { useStore } from '@/lib/store';
import { discoverProjects } from '@/api/discovery';
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { relativeTime } from '@/lib/time';

export function Settings() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const setProjectsRoot = useStore((s) => s.setProjectsRoot);

  const { data: projects, isFetching } = useQuery({
    queryKey: ['discover', projectsRoot],
    queryFn: () => discoverProjects(projectsRoot!),
    enabled: !!projectsRoot,
    staleTime: 60_000,
  });

  return (
    <div className="flex flex-col gap-4 max-w-lg">
      <Card size="sm">
        <CardHeader>
          <CardTitle className="text-sm font-medium">Diretório de projetos</CardTitle>
          <CardDescription className="text-xs text-muted-foreground">
            <code className="font-mono text-foreground">{projectsRoot ?? 'Não configurado'}</code>
          </CardDescription>
        </CardHeader>
        <div className="px-4 pb-4 flex items-center gap-2">
          <button
            className="bg-primary text-primary-foreground px-3 py-1.5 rounded text-sm"
            onClick={async () => {
              const sel = await open({ directory: true, multiple: false });
              if (typeof sel === 'string') setProjectsRoot(sel);
            }}
          >
            Selecionar pasta
          </button>
          {projectsRoot && (
            <button
              className="text-muted-foreground hover:text-foreground px-3 py-1.5 rounded text-sm border border-border"
              onClick={() => setProjectsRoot(null)}
            >
              Limpar
            </button>
          )}
        </div>
      </Card>
      {projectsRoot && (
        <Card size="sm">
          <CardHeader>
            <CardTitle className="text-sm font-medium">Projetos descobertos</CardTitle>
            <CardDescription className="text-xs text-muted-foreground">
              {isFetching ? 'Descobrindo…' : `${projects?.length ?? 0} encontrados`}
            </CardDescription>
          </CardHeader>
          {!isFetching && projects && projects.length > 0 && (
            <ul className="flex flex-col gap-0.5 text-sm px-2 pb-3">
              {projects.map((p) => (
                <li key={p.id} className="flex items-center gap-2 px-2 py-1 rounded hover:bg-muted/40">
                  <span>{p.name}</span>
                  <span className="text-muted-foreground text-xs ml-auto">
                    {p.last_activity_ms ? relativeTime(new Date(p.last_activity_ms).toISOString()) : '—'}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </Card>
      )}
    </div>
  );
}
