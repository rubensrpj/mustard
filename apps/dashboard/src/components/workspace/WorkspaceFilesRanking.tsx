import { useMemo } from "react";
import { cn } from "@/lib/utils";
import { DataCard, SectionHeader, EmptyState } from "@/components/page";
import { useWorkspaceSummarySingle } from "@/hooks/useWorkspaceSummary";

interface WorkspaceFilesRankingProps {
  repoPath: string;
}

const TOP_N = 10;
const NF = new Intl.NumberFormat("pt-BR");

/**
 * Top-N files touched today, ranked by hits. Reuses the workspace summary
 * payload (`top_files_today`) so no extra round-trip is needed.
 */
export function WorkspaceFilesRanking({ repoPath }: WorkspaceFilesRankingProps) {
  const { data, isLoading } = useWorkspaceSummarySingle(repoPath);

  const rows = useMemo(
    () => (data?.top_files_today ?? []).slice(0, TOP_N),
    [data?.top_files_today],
  );

  return (
    <DataCard padded>
      <SectionHeader
        title="Arquivos mais tocados hoje"
        right={
          <span
            className="tabular-nums"
            style={{ fontVariantNumeric: "tabular-nums" }}
          >
            top {Math.min(rows.length, TOP_N)}
          </span>
        }
      />

      {isLoading && rows.length === 0 ? (
        <p className="mt-3 text-[12.5px] text-muted-foreground/70">Carregando…</p>
      ) : rows.length === 0 ? (
        <EmptyState
          className="mt-3"
          title="Sem arquivos tocados hoje"
          description="Edits feitos via pipeline aparecem aqui à medida que acontecem."
        />
      ) : (
        <ul className="mt-3 flex flex-col">
          {rows.map((row, idx) => (
            <li
              key={`${row.path}-${idx}`}
              className={cn(
                "flex items-center justify-between gap-3 px-2 py-1.5",
                "border-b border-border/30 last:border-b-0",
              )}
            >
              <span
                // Truncate-left effect: render in RTL so the start is clipped,
                // keeping the meaningful tail (filename) visible. The inner
                // span re-asserts LTR so the text content reads naturally.
                dir="rtl"
                className="font-mono text-[12px] text-foreground/80 truncate min-w-0 flex-1 text-left"
                title={row.path}
              >
                <span dir="ltr">{row.path}</span>
              </span>
              <span
                className="text-[12px] tabular-nums text-muted-foreground shrink-0"
                style={{ fontVariantNumeric: "tabular-nums" }}
              >
                {NF.format(row.count)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </DataCard>
  );
}
