import { useMemo } from "react";
import { useNavigate } from "react-router";
import { cn } from "@/lib/utils";
import { DataCard, SectionHeader, EmptyState } from "@/components/page";
import { Badge } from "@/components/ui/badge";
import { useWorkspaceTokenSummary } from "@/hooks/useWorkspaceTokenSummary";

interface WorkspaceTokenSummaryProps {
  repoPath: string;
}

const NF = new Intl.NumberFormat("pt-BR");

/**
 * Aggregate token-savings card — big hero number on top with the top-3
 * pipelines underneath. Mirrors the spec section "Economia de tokens".
 */
export function WorkspaceTokenSummary({ repoPath }: WorkspaceTokenSummaryProps) {
  const navigate = useNavigate();
  const { data, isLoading } = useWorkspaceTokenSummary(repoPath);

  const top3 = useMemo(
    () => (data?.top_pipelines ?? []).slice(0, 3),
    [data?.top_pipelines],
  );

  const empty = !isLoading && (!data || data.total_saved === 0);

  return (
    <DataCard padded>
      <SectionHeader title="Economia de tokens" />

      {isLoading && !data ? (
        <p className="mt-3 text-[12.5px] text-muted-foreground/70">Carregando…</p>
      ) : empty ? (
        <EmptyState
          className="mt-3"
          title="Sem economia registrada"
          description="Rode pipelines com RTK ativo para começar a acumular."
        />
      ) : (
        <>
          <div className="mt-3 flex items-baseline gap-2">
            <span
              className="text-3xl font-bold tabular-nums text-[--color-accent-mustard]"
              style={{ fontVariantNumeric: "tabular-nums" }}
              aria-label={`${NF.format(data?.total_saved ?? 0)} tokens economizados nos últimos 30 dias`}
            >
              {NF.format(data?.total_saved ?? 0)}
            </span>
            <span className="text-[11px] text-muted-foreground uppercase tracking-wide">
              tokens
            </span>
          </div>
          <p className="mt-0.5 text-[12px] text-muted-foreground">últimos 30 dias</p>

          {top3.length > 0 && (
            <ul className="mt-3 flex flex-col gap-1.5">
              {top3.map((p) => (
                <li
                  key={p.spec}
                  className="flex items-center justify-between gap-2 min-w-0"
                >
                  <Badge variant="info" className="truncate max-w-[220px]" title={p.spec}>
                    {p.spec}
                  </Badge>
                  <span
                    className="text-[12.5px] tabular-nums text-foreground/80 shrink-0"
                    style={{ fontVariantNumeric: "tabular-nums" }}
                  >
                    {NF.format(p.saved)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}

      <div className="mt-3 text-right">
        <a
          href="/economia"
          onClick={(e) => {
            e.preventDefault();
            navigate("/economia");
          }}
          className={cn(
            "text-[11px] text-[--color-accent-mustard] hover:underline",
            "focus-visible:outline-none focus-visible:ring-2",
            "focus-visible:ring-[--color-accent-mustard] rounded",
          )}
        >
          Ver detalhes →
        </a>
      </div>
    </DataCard>
  );
}
