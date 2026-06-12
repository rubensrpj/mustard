// CodeViewerPanel — a docked, IDE-style (VS Code) file viewer pinned to the
// RIGHT of the layout, with a TAB BAR: open many files, switch between them,
// close them individually or all at once. Driven entirely by the global
// `useCodeViewerStore` (open tabs + active tab), so any card / row anywhere can
// `openFile(...)` and the panel reacts — there is one panel for the whole app.
//
// Renders nothing when no file is open (`tabs.length === 0`) so it costs no
// layout space until used. The heavy half (CodeBlock + Markdown + the Prism
// highlighter) lives in `CodeViewerBody`, pulled in with `React.lazy` so the
// highlighter never enters the main bundle — it loads on first open behind a
// small "carregando…" fallback (mirrors the old CodeViewer modal).
//
// MOUNT: this component is global — the layout step (next) mounts it once in a
// split beside the page content. It is self-sizing (~46% width, min 480px) and
// self-hiding; the layout just needs to render it.

import { lazy, Suspense } from "react";
import { Loader2, X, FileCode2, PanelRightClose } from "lucide-react";
import { useCodeViewerStore } from "@/lib/code-viewer-store";
import { cn } from "@/lib/utils";

const CodeViewerBody = lazy(() => import("./CodeViewerBody"));

/** One tab in the bar: file icon + truncated basename + a close "×". The active
 *  tab is highlighted (raised surface + foreground text); inactive tabs read
 *  muted and brighten on hover (VS Code spirit). */
function Tab({
  id,
  fileName,
  active,
  onSelect,
  onClose,
}: {
  id: string;
  fileName: string;
  active: boolean;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
}) {
  return (
    <div
      className={cn(
        "group flex shrink-0 items-center gap-1.5 border-r border-border/60 pl-3 pr-2 py-1.5",
        "text-[12px] transition-colors",
        active
          ? "bg-card text-foreground"
          : "bg-muted/20 text-muted-foreground hover:bg-muted/40 hover:text-foreground/90",
      )}
    >
      <button
        type="button"
        onClick={() => onSelect(id)}
        title={fileName}
        className="flex min-w-0 items-center gap-1.5 focus-visible:outline-none"
      >
        <FileCode2 className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden />
        <span className="max-w-[160px] truncate font-mono">{fileName}</span>
      </button>
      <button
        type="button"
        onClick={() => onClose(id)}
        aria-label={`fechar ${fileName}`}
        className={cn(
          "shrink-0 rounded p-0.5 text-muted-foreground/70",
          "hover:bg-muted/60 hover:text-foreground",
          // Keep the active tab's close button visible; reveal others on hover.
          active ? "opacity-100" : "opacity-0 group-hover:opacity-100",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
        )}
      >
        <X className="h-3 w-3" aria-hidden />
      </button>
    </div>
  );
}

/**
 * The docked panel. Subscribes to the tab store; renders nothing when empty.
 * Fixed width (~46% of the viewport, min 480px), a subtle left border and the
 * canvas background so it reads as a side dock, not a card.
 */
export function CodeViewerPanel() {
  const tabs = useCodeViewerStore((s) => s.tabs);
  const activeId = useCodeViewerStore((s) => s.activeId);
  const setActive = useCodeViewerStore((s) => s.setActive);
  const closeTab = useCodeViewerStore((s) => s.closeTab);
  const closeAll = useCodeViewerStore((s) => s.closeAll);

  // Empty → occupy no space at all.
  if (tabs.length === 0) return null;

  const active = tabs.find((t) => t.id === activeId) ?? tabs[0];

  return (
    <aside
      className={cn(
        "flex h-full min-h-0 w-[46%] min-w-[480px] flex-col",
        "border-l border-border/70 bg-background",
      )}
      aria-label="Visualizador de arquivos"
    >
      {/* Tab bar */}
      <div className="flex items-stretch border-b border-border/60 bg-muted/10">
        <div className="flex min-w-0 flex-1 items-stretch overflow-x-auto">
          {tabs.map((t) => (
            <Tab
              key={t.id}
              id={t.id}
              fileName={t.fileName}
              active={t.id === active.id}
              onSelect={setActive}
              onClose={closeTab}
            />
          ))}
        </div>
        <button
          type="button"
          onClick={closeAll}
          aria-label="fechar todas as abas"
          title="Fechar todas"
          className={cn(
            "flex shrink-0 items-center justify-center px-2",
            "text-muted-foreground/80 transition-colors hover:bg-muted/40 hover:text-foreground",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
          )}
        >
          <PanelRightClose className="h-4 w-4" aria-hidden />
        </button>
      </div>

      {/* Active tab body (lazy highlighter). Keyed by tab id so a tab switch
          remounts the body with the new file's content. */}
      <div className="min-h-0 flex-1">
        <Suspense
          fallback={
            <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
              <Loader2 className="h-5 w-5 animate-spin" aria-hidden />
              <p className="text-[12px]">Carregando…</p>
            </div>
          }
        >
          <CodeViewerBody key={active.id} tab={active} />
        </Suspense>
      </div>
    </aside>
  );
}
