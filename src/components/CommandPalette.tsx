import { useEffect, useState, useCallback } from "react";
import { useNavigate } from "react-router";
import { Command } from "cmdk";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { useTheme } from "@/hooks/useTheme";
import { useStore } from "@/lib/store";
import { queryClient } from "@/lib/query-client";
import type { Project } from "@/api/discovery";
import type { SpecRow } from "@/lib/dashboard";

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const { theme, setTheme } = useTheme();
  const projectsRoot = useStore((s) => s.projectsRoot);
  const selectedProjectId = useStore((s) => s.selectedProjectId);
  const setSelectedProjectId = useStore((s) => s.setSelectedProjectId);
  const projects = queryClient.getQueryData<Project[]>(['discover', projectsRoot]) ?? [];
  const selectedProject = selectedProjectId
    ? projects.find((p) => p.id === selectedProjectId) ?? null
    : null;
  const specsForSelected: SpecRow[] = selectedProject
    ? queryClient.getQueryData<SpecRow[]>(['specs', selectedProject.path]) ?? []
    : [];

  useEffect(() => {
    function handler(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    }
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  const run = useCallback((fn: () => void) => {
    fn();
    setOpen(false);
  }, []);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        showCloseButton={false}
        className="p-0 max-w-[min(560px,90vw)] overflow-hidden border border-border bg-card shadow-2xl gap-0"
      >
        <DialogTitle className="sr-only">Command palette</DialogTitle>
        <Command label="Command palette" loop>
          <Command.Input
            autoFocus
            placeholder="Type a command or search…"
            className="w-full px-4 py-3 bg-transparent border-b border-border text-sm outline-none placeholder:text-muted-foreground text-foreground"
          />
          <Command.List className="max-h-[320px] overflow-y-auto p-1">
            <Command.Empty className="px-3 py-6 text-center text-sm text-muted-foreground">
              Sem resultados.
            </Command.Empty>
            <Command.Group
              heading="Navegar"
              className="text-[10px] uppercase tracking-wider text-muted-foreground px-2 py-1 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-muted-foreground"
            >
              <Command.Item
                onSelect={() => run(() => navigate("/"))}
                className="px-3 py-2 rounded-md text-sm cursor-pointer text-foreground data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary"
              >
                Ir para Home
              </Command.Item>
              <Command.Item
                onSelect={() => run(() => navigate("/knowledge"))}
                className="px-3 py-2 rounded-md text-sm cursor-pointer text-foreground data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary"
              >
                Ir para Knowledge
              </Command.Item>
              <Command.Item
                onSelect={() => run(() => navigate("/activity"))}
                className="px-3 py-2 rounded-md text-sm cursor-pointer text-foreground data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary"
              >
                Ir para Activity
              </Command.Item>
              <Command.Item
                onSelect={() => run(() => navigate("/settings"))}
                className="px-3 py-2 rounded-md text-sm cursor-pointer text-foreground data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary"
              >
                Ir para Settings
              </Command.Item>
            </Command.Group>
            {projects.length > 0 && (
              <Command.Group
                heading="Projetos"
                className="text-[10px] uppercase tracking-wider text-muted-foreground px-2 py-1 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-muted-foreground"
              >
                {projects.map((p) => (
                  <Command.Item
                    key={p.id}
                    value={`switch-${p.id}-${p.name}`}
                    onSelect={() => run(() => { setSelectedProjectId(p.id); navigate(`/project/${p.id}`); })}
                    className="px-3 py-2 rounded-md text-sm cursor-pointer text-foreground data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary"
                  >
                    Switch to {p.name}
                  </Command.Item>
                ))}
              </Command.Group>
            )}
            {selectedProject && specsForSelected.length > 0 && (
              <Command.Group
                heading="Specs"
                className="text-[10px] uppercase tracking-wider text-muted-foreground px-2 py-1 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-muted-foreground"
              >
                {specsForSelected.map((s) => (
                  <Command.Item
                    key={s.name}
                    value={`spec-${selectedProject.id}-${s.name}`}
                    onSelect={() =>
                      run(() =>
                        navigate(
                          `/project/${selectedProject.id}/spec/${encodeURIComponent(s.name)}`,
                        ),
                      )
                    }
                    className="px-3 py-2 rounded-md text-sm cursor-pointer text-foreground data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary"
                  >
                    Open spec: {s.name}
                  </Command.Item>
                ))}
              </Command.Group>
            )}
            <Command.Group
              heading="Tema"
              className="text-[10px] uppercase tracking-wider text-muted-foreground px-2 py-1 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-muted-foreground"
            >
              {theme === "dark" ? (
                <Command.Item
                  onSelect={() => run(() => setTheme("light"))}
                  className="px-3 py-2 rounded-md text-sm cursor-pointer text-foreground data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary"
                >
                  Mudar para tema claro
                </Command.Item>
              ) : (
                <Command.Item
                  onSelect={() => run(() => setTheme("dark"))}
                  className="px-3 py-2 rounded-md text-sm cursor-pointer text-foreground data-[selected=true]:bg-primary/10 data-[selected=true]:text-primary"
                >
                  Mudar para tema escuro
                </Command.Item>
              )}
            </Command.Group>
          </Command.List>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
