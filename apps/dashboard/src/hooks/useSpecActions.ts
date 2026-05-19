import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  completeSpec,
  cancelSpec,
  reactivateSpec,
  type SpecBucket,
} from "@/lib/dashboard";

function invalidateSpecs(
  queryClient: ReturnType<typeof useQueryClient>,
  repoPath: string,
) {
  queryClient.invalidateQueries({ queryKey: ["specs", repoPath] });
  queryClient.invalidateQueries({ queryKey: ["active-pipelines", repoPath] });
}

export function useSpecActions(repoPath: string | undefined) {
  const queryClient = useQueryClient();

  const complete = useMutation({
    mutationFn: (specName: string): Promise<SpecBucket> => {
      if (!repoPath) return Promise.reject(new Error("Sem projeto selecionado"));
      return completeSpec(repoPath, specName);
    },
    onSuccess: (_b, specName) => {
      toast.success(`Spec ${specName} concluída`);
      if (repoPath) invalidateSpecs(queryClient, repoPath);
    },
    onError: (err: unknown, specName) => {
      const msg = err instanceof Error ? err.message : String(err);
      toast.error(`Falha ao concluir ${specName}: ${msg}`);
    },
  });

  const cancel = useMutation({
    mutationFn: (specName: string): Promise<SpecBucket> => {
      if (!repoPath) return Promise.reject(new Error("Sem projeto selecionado"));
      return cancelSpec(repoPath, specName);
    },
    onSuccess: (_b, specName) => {
      toast.success(`Spec ${specName} cancelada`);
      if (repoPath) invalidateSpecs(queryClient, repoPath);
    },
    onError: (err: unknown, specName) => {
      const msg = err instanceof Error ? err.message : String(err);
      toast.error(`Falha ao cancelar ${specName}: ${msg}`);
    },
  });

  const reactivate = useMutation({
    mutationFn: (specName: string): Promise<SpecBucket> => {
      if (!repoPath) return Promise.reject(new Error("Sem projeto selecionado"));
      return reactivateSpec(repoPath, specName);
    },
    onSuccess: (_b, specName) => {
      toast.success(`Spec ${specName} reativada`);
      if (repoPath) invalidateSpecs(queryClient, repoPath);
    },
    onError: (err: unknown, specName) => {
      const msg = err instanceof Error ? err.message : String(err);
      toast.error(`Falha ao reativar ${specName}: ${msg}`);
    },
  });

  return { complete, cancel, reactivate };
}
