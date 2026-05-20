import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { dashboardSpecAction, type SpecAction, type SpecActionKind } from "@/lib/dashboard";

interface SpecActionVars {
  spec: string;
  action: SpecActionKind;
}

export function useSpecAction(repoPath: string | null) {
  const queryClient = useQueryClient();

  return useMutation<SpecAction, Error, SpecActionVars>({
    mutationFn: ({ spec, action }) => {
      if (!repoPath) return Promise.reject(new Error("Sem projeto selecionado"));
      return dashboardSpecAction(repoPath, spec, action);
    },
    onSuccess: (result, { spec }) => {
      const label =
        result.action === "reopen"
          ? "reaberta"
          : result.action === "close"
            ? "fechada"
            : "removida";
      toast.success(`Spec ${spec} ${label}`);

      // Invalidate affected query families
      queryClient.invalidateQueries({ queryKey: ["spec-card"] });
      queryClient.invalidateQueries({ queryKey: ["workspace-summary"] });
      queryClient.invalidateQueries({ queryKey: ["specs-list"] });
      if (repoPath) {
        queryClient.invalidateQueries({ queryKey: ["specs", repoPath] });
        queryClient.invalidateQueries({ queryKey: ["active-pipelines", repoPath] });
      }
    },
    onError: (err, { spec }) => {
      toast.error(`Falha na ação sobre ${spec}: ${err.message}`);
    },
  });
}
