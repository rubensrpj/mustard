import { useState } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useSpecAction } from "@/hooks/useSpecAction";
import { SpecActionConfirm } from "../SpecActionConfirm";

interface SpecActionMenuProps {
  repoPath: string | null;
  spec: string;
  /** Current status — drives which actions are available. */
  status: string;
}

export function SpecActionMenu({ repoPath, spec, status }: SpecActionMenuProps) {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const mutation = useSpecAction(repoPath);

  const isClosed = ["completed", "closed", "cancelled"].includes(status);

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
              Reabrir
            </DropdownMenuItem>
          ) : (
            <DropdownMenuItem onClick={handleClose} disabled={mutation.isPending}>
              Fechar
            </DropdownMenuItem>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem
            onClick={() => setConfirmOpen(true)}
            className="text-[--intent-error] focus:text-[--intent-error]"
            disabled={mutation.isPending}
          >
            Remover
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
