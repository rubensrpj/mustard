// CodeViewer — presentational modal that shows ONE file with real syntax
// highlighting (markdown files render via the rich Markdown component).
//
// LAZY: the heavy half (`CodeViewerContent` → CodeBlock → react-syntax-
// highlighter + Markdown) is pulled in with `React.lazy`, so the highlighter
// never enters the main bundle. While closed, the lazy chunk is not fetched at
// all; on first open it loads behind a small "carregando…" fallback. This is
// the foundation (etapa 1) — the data source (Git / most-touched / README /
// tracer) is wired in a following step; here the component is purely driven by
// the props it receives.

import { lazy, Suspense } from "react";
import { Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import type { CodeViewerProps } from "./types";

export type { CodeViewerProps } from "./types";

const CodeViewerContent = lazy(() => import("./CodeViewerContent"));

/** Minimal fallback shown while the highlighter chunk loads on first open.
 *  Rendered inside its own Dialog so the modal frame appears instantly and the
 *  body fills in once the lazy chunk arrives. */
function LoadingFallback({
  open,
  onOpenChange,
  fileName,
}: Pick<CodeViewerProps, "open" | "onOpenChange" | "fileName">) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="!max-w-[min(96vw,1100px)] !w-[min(96vw,1100px)] h-[90vh] p-0 gap-0 overflow-hidden">
        <DialogTitle className="sr-only">{fileName}</DialogTitle>
        <DialogDescription className="sr-only">Carregando visualizador</DialogDescription>
        <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
          <Loader2 className="h-5 w-5 animate-spin" aria-hidden />
          <p className="text-[12px]">Carregando…</p>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function CodeViewer(props: CodeViewerProps) {
  // Don't mount (or fetch) the lazy chunk until the viewer is opened.
  if (!props.open) return null;
  return (
    <Suspense
      fallback={
        <LoadingFallback
          open={props.open}
          onOpenChange={props.onOpenChange}
          fileName={props.fileName}
        />
      }
    >
      <CodeViewerContent {...props} />
    </Suspense>
  );
}
