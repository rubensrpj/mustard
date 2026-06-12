import { useMemo } from "react";
import { FileCode } from "lucide-react";
import { cn } from "@/lib/utils";
import { DataCard, SectionHeader, EmptyState } from "@/components/page";
import { StatPill } from "@/components/page";
import { useWorkspaceSummarySingle } from "@/hooks/useWorkspaceSummary";
import { useCodeViewerStore } from "@/lib/code-viewer-store";
import { useTranslate } from "@/lib/i18n";

interface WorkspaceFilesRankingProps {
  repoPath: string;
}

const TOP_N = 10;

/**
 * Top-N files touched today, ranked by hits. Consumes `top_files_today` from
 * the workspace summary; Wave 8 (2026-05-21) flipped the source to a
 * session-agnostic SQL aggregation on the Rust side so this list keeps
 * populating across spec CLOSEs.
 *
 * Visual: each row now leads with a `FileCode` icon and renders the hit count
 * as a `<StatPill>` so the dense list reads consistently with the rest of
 * the design system (W5 primitives).
 */
export function WorkspaceFilesRanking({ repoPath }: WorkspaceFilesRankingProps) {
  const t = useTranslate();
  const { data, isLoading } = useWorkspaceSummarySingle(repoPath);
  // Top-files paths may arrive ABSOLUTE (e.g. `C:\Atiz\sialia\…`); pass them
  // through as-is — `dashboard_read_file` resolves absolutes inside the repo.
  // Opens into the global docked CodeViewerPanel (one panel for the whole app).
  const openFile = useCodeViewerStore((s) => s.openFile);

  const rows = useMemo(
    () => (data?.top_files_today ?? []).slice(0, TOP_N),
    [data?.top_files_today],
  );

  return (
    <DataCard padded>
      <SectionHeader
        title={t("workspace.filesRanking")}
        right={
          <span
            className="text-[11px] text-muted-foreground tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            top {Math.min(rows.length, TOP_N)}
          </span>
        }
      />

      {isLoading && rows.length === 0 ? (
        <p className="mt-3 text-[12.5px] text-muted-foreground/70">{t("common.loading")}</p>
      ) : rows.length === 0 ? (
        <EmptyState
          className="mt-3"
          title={t("common.empty")}
          description="Edits feitos via pipeline aparecem aqui à medida que acontecem."
        />
      ) : (
        <ul className="mt-3 flex flex-col">
          {rows.map((row, idx) => (
            <li
              key={`${row.path}-${idx}`}
              className="border-b border-border/30 last:border-b-0"
            >
              <button
                type="button"
                onClick={() => openFile(repoPath, row.path)}
                aria-label={`abrir ${row.path}`}
                title={row.path}
                className={cn(
                  "flex w-full items-center gap-3 rounded-md px-2 py-1.5 text-left",
                  "transition-colors hover:bg-muted/30",
                  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
                )}
              >
                <FileCode
                  className="h-3.5 w-3.5 shrink-0 text-[--ds-text-tertiary]"
                  aria-hidden
                />
                <span
                  // Truncate-left effect: render in RTL so the start is clipped,
                  // keeping the meaningful tail (filename) visible. The inner
                  // span re-asserts LTR so the text content reads naturally.
                  dir="rtl"
                  className="font-mono text-[12px] text-foreground/80 truncate min-w-0 flex-1 text-left"
                >
                  <span dir="ltr">{row.path}</span>
                </span>
                <StatPill value={row.count} unit="hit" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </DataCard>
  );
}
