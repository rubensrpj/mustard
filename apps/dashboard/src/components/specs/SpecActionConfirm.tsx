import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

interface SpecActionConfirmProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  spec: string;
  onConfirm: () => void;
}

/**
 * Confirmation modal for the destructive "Remover" action.
 * Uses Radix Dialog via the project's shadcn Dialog primitive.
 */
export function SpecActionConfirm({
  open,
  onOpenChange,
  spec,
  onConfirm,
}: SpecActionConfirmProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Remover spec</DialogTitle>
          <DialogDescription>
            Tem certeza que deseja remover a spec{" "}
            <strong className="font-mono text-foreground">{spec}</strong>?{" "}
            Esta ação não pode ser desfeita.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => onOpenChange(false)}
            type="button"
          >
            Cancelar
          </Button>
          <Button
            variant="destructive"
            onClick={onConfirm}
            type="button"
          >
            Remover
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
