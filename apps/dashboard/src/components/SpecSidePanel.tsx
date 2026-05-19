import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { X } from "lucide-react";
import { fetchSpecMarkdown, type SpecRow } from "@/lib/dashboard";
import { resolveWaveFamily } from "@/lib/waves";
import { Markdown } from "@/components/Markdown";
import { WaveNav } from "@/components/WaveNav";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

type SpecSidePanelProps = {
  projectId: string | null;
  projectPath: string | null;
  /** The spec the user clicked — parent plan or a wave child. */
  spec: SpecRow;
  /** All specs of the project, used to resolve the wave-plan family. */
  allSpecs: SpecRow[];
  onClose: () => void;
};

/**
 * Conteúdo do painel de detalhe de uma spec. Renderizado dentro do
 * `SplitDetail` (50/50) — por isso é só header + corpo, sem wrapper próprio.
 *
 * Quando a spec é um wave plan (decomposta em ondas), uma faixa `WaveNav`
 * permite alternar entre a spec pai e o `spec.md` de cada wave sem fechar
 * o painel.
 */
export function SpecSidePanel({
  projectId,
  projectPath,
  spec,
  allSpecs,
  onClose,
}: SpecSidePanelProps) {
  const family = resolveWaveFamily(allSpecs, spec.name);

  // Qual membro da família está em exibição. Reseta quando o usuário clica
  // numa spec diferente na tabela.
  const [viewing, setViewing] = useState(spec.name);
  useEffect(() => setViewing(spec.name), [spec.name]);

  const viewingRow = allSpecs.find((s) => s.name === viewing) ?? spec;

  const { data: markdown, isLoading, error } = useQuery({
    queryKey: ["spec-markdown", projectPath, viewing],
    queryFn: () => fetchSpecMarkdown(projectPath!, viewing),
    enabled: !!projectPath && !!viewing,
    staleTime: 60_000,
  });

  // An empty or whitespace-only string is not "content" — treat it like a
  // missing spec so the panel shows an explanatory message, not a blank body.
  const hasContent = typeof markdown === "string" && markdown.trim().length > 0;

  return (
    <>
      <header className="px-5 py-4 border-b border-border flex flex-col gap-2">
        <div className="flex items-start gap-2">
          <span
            className="font-mono text-sm font-medium truncate flex-1"
            title={viewing}
          >
            {viewing}
          </span>
          <Button
            variant="ghost"
            size="sm"
            onClick={onClose}
            className="h-7 w-7 p-0 -mt-1"
            title="Fechar"
          >
            <X className="size-3.5" />
          </Button>
        </div>
        <div className="flex items-center gap-2 flex-wrap">
          {viewingRow.phase && (
            <Badge variant="secondary" className="text-[11px] py-0">
              {viewingRow.phase}
            </Badge>
          )}
          {viewingRow.status && (
            <Badge variant="outline" className="text-[11px] py-0">
              {viewingRow.status}
            </Badge>
          )}
          {projectId && (
            <Link
              to={`/project/${projectId}/spec/${encodeURIComponent(viewing)}`}
              onClick={onClose}
              className="ml-auto text-[12px] text-primary hover:underline underline-offset-2 whitespace-nowrap"
            >
              Abrir página completa →
            </Link>
          )}
        </div>
      </header>

      {family.isWavePlan && (
        <WaveNav
          parentName={family.parentName}
          waves={family.waves}
          current={viewing}
          onSelect={setViewing}
        />
      )}

      <div className="flex-1 overflow-y-auto px-5 py-4">
        {isLoading && (
          <div className="flex flex-col gap-2">
            {[0, 1, 2, 3, 4].map((i) => (
              <div key={i} className="h-4 bg-muted/40 rounded animate-pulse" />
            ))}
          </div>
        )}
        {error && !isLoading && (
          <div className="flex flex-col gap-1.5">
            <p className="text-[13px] text-destructive">
              Não foi possível carregar o detalhe desta spec.
            </p>
            <p className="text-[12px] text-muted-foreground">
              {/* The Tauri command returns this when no spec.md exists at the
                  expected path — surfacing it explicitly beats a mute panel. */}
              {(error as Error).message}
            </p>
          </div>
        )}
        {!isLoading && !error && hasContent && (
          <Markdown content={markdown!} />
        )}
        {!isLoading && !error && !hasContent && (
          <p className="text-[13px] text-muted-foreground">
            Esta spec não tem um <code className="font-mono">spec.md</code> com conteúdo.
          </p>
        )}
      </div>
    </>
  );
}
