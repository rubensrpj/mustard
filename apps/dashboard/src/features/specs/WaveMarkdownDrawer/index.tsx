import { AlertCircle, FileText } from "lucide-react";
import { Markdown } from "@/components/page/Markdown";
import { useSpecChecklistProgress } from "@/hooks/useSpecChecklistProgress";
import { useSpecWaveFiles } from "@/hooks/useSpecWaveFiles";
import type { WaveChecklistProgress } from "@/lib/dashboard";

interface WaveMarkdownDrawerProps {
  repoPath: string | null;
  spec: string;
  /** When `null` the panel shows the empty prompt; otherwise it loads the
   *  `wave-N-{role}/spec.md` markdown for the selected wave. */
  wave: number | null;
  /** Optional wave role shown in the header (e.g. "ui", "backend"). */
  role?: string | null;
  /** Whether the selected wave is CONCLUDED (status `completed`). When true the
   *  `## Tarefas` checklist renders all boxes marked even if the on-disk
   *  markdown still reads `[ ]` (the real progress lives in events/meta). */
  waveCompleted?: boolean;
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
 * Header chrome for the wave markdown panel. The pin / close affordances were
 * removed when the panel became an always-open resizable split (spec
 * `melhorias-no-dashboard-destacar-projeto`, wave 2): the panel is no longer
 * a dismissible overlay, so the only header content is the wave label + the
 * optional checklist badge.
 */
function DrawerHeader({
  label,
  checklist,
}: {
  label: string;
  /** `N/M itens` badge for this wave. `null` when the wave has no checklist
   *  data — nothing is rendered. */
  checklist?: WaveChecklistProgress | null;
}) {
  return (
    <div className="flex items-center gap-2 px-5 pt-5 pb-3 border-b border-border/60">
      <FileText
        className="h-4 w-4 text-muted-foreground shrink-0"
        aria-hidden
      />
      <span className="font-mono text-[13px] truncate flex-1 min-w-0">
        {label}
      </span>
      {checklist && (checklist.total > 0 || checklist.done > 0) && (
        <span
          className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-muted text-muted-foreground tabular-nums shrink-0"
          style={{ fontVariantNumeric: "tabular-nums" }}
          title="Itens do checklist concluídos nesta onda (meta.json + eventos checklist.item.marked)"
        >
          {checklist.total > 0
            ? `${checklist.done}/${checklist.total} itens`
            : `itens marcados: ${checklist.done}`}
        </span>
      )}
    </div>
  );
}

/**
 * Wave markdown panel — renders the full `wave-N-{role}/spec.md` markdown for
 * the selected wave. Backed by `useSpecWaveFiles`, which also drives the
 * real-file-count badge in `SpecWavesTab`; React Query dedupes both reads by
 * the shared queryKey.
 *
 * Spec `melhorias-no-dashboard-destacar-projeto` (wave 2): this is now a plain
 * panel — the legacy `<Sheet>` overlay and the pin/overlay toggle were
 * removed. The wave detail lives inside an always-open resizable split owned
 * by `SpecWavesTab`, so this component only renders the header + body and lets
 * the split own the container chrome.
 *
 * Reuses the shared `<Markdown>` renderer (already v10-correct: overrides
 * `pre` separately and detects code blocks via className/newline rather than
 * the removed `inline` prop on `code`).
 */
export function WaveMarkdownDrawer({
  repoPath,
  spec,
  wave,
  role,
  waveCompleted = false,
}: WaveMarkdownDrawerProps) {
  const waveNum = wave ?? 0;
  // `enabled` inside `useSpecWaveFiles` requires `wave >= 0`, so we only
  // pass through when the parent has actually selected a wave. Passing 0
  // for the closed state would fire a spurious "Onda #0" fetch.
  const q = useSpecWaveFiles(repoPath, spec, wave == null ? -1 : waveNum);

  // Per-wave checklist progress for the header badge. Same queryKey as
  // `SpecWavesTab`'s query — React Query dedupes the round-trip. `null` when
  // the open wave carries no checklist data.
  const checklistQ = useSpecChecklistProgress(repoPath, spec || null);
  const checklist =
    wave == null
      ? null
      : ((checklistQ.data ?? []).find((r) => r.wave === wave) ?? null);

  const headerLabel =
    wave == null
      ? "Wave"
      : wave === 0
        ? `Onda #0 — ${role ?? "spec principal"}`
        : `Wave ${wave} — ${role ?? "…"}`;

  const body = (
    <div className="flex-1 overflow-auto">
      {wave == null ? (
        <ErrorState message="Selecione uma onda para ver o conteúdo." />
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
          <Markdown content={q.data.markdown} forceChecked={waveCompleted} />
        </div>
      ) : (
        <ErrorState message="Esta onda não tem markdown próprio." />
      )}
    </div>
  );

  return (
    <div className="flex flex-col h-full min-h-0">
      <DrawerHeader label={headerLabel} checklist={checklist} />
      {body}
    </div>
  );
}
