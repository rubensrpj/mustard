import { AlertCircle, FileText, Pin, PinOff, X } from "lucide-react";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { cn } from "@/lib/utils";
import { Markdown } from "@/components/page/Markdown";
import { useSpecWaveFiles } from "@/hooks/useSpecWaveFiles";

interface WaveMarkdownDrawerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  repoPath: string | null;
  spec: string;
  /** When `null` the drawer is closed; the parent controls open via this. */
  wave: number | null;
  /** Optional wave role shown in the header (e.g. "ui", "backend"). */
  role?: string | null;
  /**
   * Wave 2 (spec `2026-05-21-dashboard-spec-tabs-polish`): when `true` the
   * drawer renders inline as an `<aside>` (no overlay) so the user sees the
   * waves list and the markdown side-by-side inside the Ondas panel. When
   * `false` (default) the drawer keeps the legacy `<Sheet>` overlay
   * behaviour. The toggle button in the header calls `onPinChange`.
   */
  pinned?: boolean;
  /** Toggle the pinned/overlay mode. Required when `pinned` is provided. */
  onPinChange?: (pinned: boolean) => void;
}

function LoadingSkeleton() {
  return (
    <ul className="flex flex-col gap-2 px-5 py-4">
      {[0, 1, 2, 3, 4].map((i) => (
        <li
          key={i}
          className="h-4 rounded bg-muted/40 animate-pulse"
          style={{ width: `${55 + ((i * 13) % 35)}%` }}
        />
      ))}
    </ul>
  );
}

function ErrorState({ message }: { message: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-10 px-4 text-center text-muted-foreground">
      <AlertCircle className="h-5 w-5 text-muted-foreground/70" aria-hidden />
      <p className="text-[13px]">{message}</p>
    </div>
  );
}

/**
 * Header chrome shared between the Sheet (overlay) and `<aside>` (inline)
 * renderings. Centralised so the Pin / close affordances stay in lockstep
 * regardless of which container the drawer is mounted in.
 */
function DrawerHeader({
  label,
  pinned,
  onPinChange,
  onClose,
  showClose,
}: {
  label: string;
  pinned: boolean;
  onPinChange?: (p: boolean) => void;
  onClose?: () => void;
  showClose: boolean;
}) {
  const PinIcon = pinned ? PinOff : Pin;
  const pinTitle = pinned ? "Soltar como janela" : "Fixar dentro do painel";
  return (
    <div className="flex items-center gap-2 px-5 pt-5 pb-3 border-b border-border/60">
      <FileText
        className="h-4 w-4 text-muted-foreground shrink-0"
        aria-hidden
      />
      <span className="font-mono text-[13px] truncate flex-1 min-w-0">
        {label}
      </span>
      {onPinChange && (
        <button
          type="button"
          onClick={() => onPinChange(!pinned)}
          aria-label={pinTitle}
          title={pinTitle}
          className="h-6 w-6 flex items-center justify-center rounded text-muted-foreground hover:text-foreground hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary] transition-colors"
        >
          <PinIcon className="h-3.5 w-3.5" aria-hidden />
        </button>
      )}
      {showClose && onClose && (
        <button
          type="button"
          onClick={onClose}
          aria-label="Fechar"
          title="Fechar"
          className="h-6 w-6 flex items-center justify-center rounded text-muted-foreground hover:text-foreground hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary] transition-colors"
        >
          <X className="h-3.5 w-3.5" aria-hidden />
        </button>
      )}
    </div>
  );
}

/**
 * Wave 2 (2026-05-21, spec `2026-05-21-dashboard-spec-tabs`) — right-side
 * drawer that renders the full `wave-N-{role}/spec.md` markdown for the
 * clicked wave row. Backed by `useSpecWaveFiles`, which also drives the
 * real-file-count badge in `SpecWavesTab`; React Query dedupes both reads
 * by the shared queryKey.
 *
 * Wave 2 (spec `2026-05-21-dashboard-spec-tabs-polish`) adds a pin toggle:
 * when `pinned=true` the drawer renders inline as a sibling `<aside>` of the
 * waves list (no overlay), and the Sheet primitive is bypassed entirely.
 *
 * Reuses the shared `<Markdown>` renderer (already v10-correct: overrides
 * `pre` separately and detects code blocks via className/newline rather than
 * the removed `inline` prop on `code`).
 */
export function WaveMarkdownDrawer({
  open,
  onOpenChange,
  repoPath,
  spec,
  wave,
  role,
  pinned = false,
  onPinChange,
}: WaveMarkdownDrawerProps) {
  const waveNum = wave ?? 0;
  // `enabled` inside `useSpecWaveFiles` requires `wave >= 0`, so we only
  // pass through when the parent has actually selected a wave. Passing 0
  // for the closed state would fire a spurious "Onda #0" fetch.
  const q = useSpecWaveFiles(repoPath, spec, wave == null ? -1 : waveNum);

  const headerLabel =
    wave == null
      ? "Wave"
      : wave === 0
        ? `Onda #0 — ${role ?? "spec principal"}`
        : `Wave ${wave} — ${role ?? "…"}`;

  const body = (
    <div className="flex-1 overflow-auto">
      {wave == null ? (
        <ErrorState message="Nenhuma onda selecionada." />
      ) : q.isLoading ? (
        <LoadingSkeleton />
      ) : q.error ? (
        <ErrorState
          message={`Erro ao carregar markdown: ${
            q.error instanceof Error ? q.error.message : String(q.error)
          }`}
        />
      ) : q.data && q.data.markdown.length > 0 ? (
        <div className="px-5 py-4">
          <Markdown content={q.data.markdown} />
        </div>
      ) : (
        <ErrorState message="Esta onda não tem markdown próprio." />
      )}
    </div>
  );

  // Inline mode: render as a sibling `<aside>`. The caller is responsible
  // for placing this next to the waves list in a flex/grid layout. We
  // intentionally do NOT use the Sheet primitive here — the overlay/focus
  // trap would fight against the side-by-side UX.
  if (pinned) {
    if (!open || wave == null) return null;
    return (
      <aside
        className={cn(
          "flex flex-col rounded-md border border-border bg-card/30",
          "min-h-[40vh] max-h-[75vh]",
        )}
        aria-label={`Markdown da wave ${wave}`}
      >
        <DrawerHeader
          label={headerLabel}
          pinned={pinned}
          onPinChange={onPinChange}
          onClose={() => onOpenChange(false)}
          showClose
        />
        {body}
      </aside>
    );
  }

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="!w-[40rem] !max-w-[95vw] flex flex-col">
        <SheetHeader className="p-0">
          <DrawerHeader
            label={headerLabel}
            pinned={pinned}
            onPinChange={onPinChange}
            showClose={false}
          />
          <SheetTitle className="sr-only">{headerLabel}</SheetTitle>
          <SheetDescription className="sr-only">
            Markdown completo da onda {wave ?? ""} da spec {spec}
          </SheetDescription>
        </SheetHeader>
        {body}
      </SheetContent>
    </Sheet>
  );
}
