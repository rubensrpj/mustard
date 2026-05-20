import { useQuery } from "@tanstack/react-query";
import { useStore } from "@/lib/store";
import { useProjects, fetchTelemetry } from "@/lib/dashboard";
import { useTelemetryPhases } from "@/hooks/useTelemetryPhases";
import { usePromptEconomy } from "@/hooks/usePromptEconomy";
import { PageHeader, EmptyState } from "@/components/page";
import { EconomySection } from "@/components/telemetry/EconomySection";

export function Economia() {
  const projectsRoot = useStore((s) => s.projectsRoot);
  const activeWorkspaceId = useStore((s) => s.activeWorkspaceId);
  const projects = useProjects();
  const activeProject = projects.find((p) => p.id === activeWorkspaceId) ?? null;
  const repoPath = activeProject?.path ?? null;

  const telemetry = useQuery({
    queryKey: ["telemetry", repoPath],
    queryFn: () => fetchTelemetry(repoPath!),
    enabled: !!repoPath,
    staleTime: 30_000,
    refetchInterval: 30_000,
  });

  const phases = useTelemetryPhases(repoPath, "all");
  const promptEconomy = usePromptEconomy(repoPath);

  if (!projectsRoot) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader
          breadcrumb={[{ label: "Workspace" }, { label: "Economia" }]}
          title="Economia"
          subtitle="Tokens, cache, e economia agregada"
        />
        <EmptyState
          title="Diretório de projetos não configurado"
          description="Vá em Configurações e aponte para a pasta onde estão seus repos."
        />
      </div>
    );
  }

  if (!activeWorkspaceId) {
    return (
      <div className="flex flex-col gap-6 w-full">
        <PageHeader
          breadcrumb={[{ label: "Workspace" }, { label: "Economia" }]}
          title="Economia"
          subtitle="Tokens, cache, e economia agregada"
        />
        <EmptyState
          title="Selecione um workspace"
          description="Use o seletor na sidebar para escolher um projeto."
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6 w-full">
      <PageHeader
        breadcrumb={[{ label: "Workspace" }, { label: "Economia" }]}
        title="Economia"
        subtitle="Tokens, cache, e economia agregada"
      />
      <EconomySection
        rtk={
          telemetry.data?.rtk ?? {
            available: false,
            total_commands: null,
            input_tokens: null,
            output_tokens: null,
            tokens_saved: null,
            savings_pct: null,
            total_exec_time_ms: null,
            daily: [],
          }
        }
        measured={telemetry.data?.measured ?? { tokens_total: 0, tokens_today: 0 }}
        prevention={telemetry.data?.prevention ?? []}
        routing={
          telemetry.data?.routing ?? {
            blocks: 0,
            allows: 0,
            by_intent: [],
            by_note: [],
            session_blocks: 0,
            session_allows: 0,
          }
        }
        phases={phases.data ?? []}
        promptEconomy={promptEconomy.data}
      />
    </div>
  );
}
