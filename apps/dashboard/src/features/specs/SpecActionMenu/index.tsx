import { useState } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useSpecAction } from "@/hooks/useSpecAction";
import { useT } from "@/lib/i18n";
import { stateFromStatus } from "../_shared/stage-from-status";
import { SpecActionConfirm } from "../SpecActionConfirm";

interface SpecActionMenuProps {
  repoPath: string | null;
  spec: string;
  /** Current status — drives which actions are available. */
  status: string;
}

export function SpecActionMenu({ repoPath, spec, status }: SpecActionMenuProps) {
  const t = useT();
  const [confirmOpen, setConfirmOpen] = useState(false);
  const mutation = useSpecAction(repoPath);

  // "Reabrir" is the inverse of "Fechar": it only makes sense for a spec that
  // already reached a terminal outcome (completed / cancelled / abandoned /
  // superseded / absorbed). Derive that from the canonical `stateFromStatus`
  // projection (the same one the page uses to bucket "Encerradas") instead of
  // a hand-kept status allow-list, so every terminal outcome is covered.
  const isClosed = stateFromStatus(status).outcome !== "active";

  function handleReopen() {
    mutation.mutate({ spec, action: "reopen" });
  }
  function handleClose() {
    mutation.mutate({ spec, action: "close" });
  }

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            aria-label={`Ações para spec ${spec}`}
            className="h-6 w-6 flex items-center justify-center rounded text-muted-foreground hover:text-foreground hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary] transition-colors opacity-0 group-hover/speccard:opacity-100 focus-visible:opacity-100"
          >
            ⋮
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-40">
          {isClosed ? (
            <DropdownMenuItem onClick={handleReopen} disabled={mutation.isPending}>
              {t("specs.action.reopen")}
            </DropdownMenuItem>
          ) : (
            <DropdownMenuItem onClick={handleClose} disabled={mutation.isPending}>
              {t("specs.action.close")}
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem
            onClick={() => setConfirmOpen(true)}
            className="text-[--intent-error] focus:text-[--intent-error]"
            disabled={mutation.isPending}
          >
            {t("specs.action.remove")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <SpecActionConfirm
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        spec={spec}
        onConfirm={() => {
          setConfirmOpen(false);
          mutation.mutate({ spec, action: "remove" });
        }}
      />
    </>
  );
}
