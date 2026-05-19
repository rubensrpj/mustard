import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { fetchSpecMarkdown, type SpecRow } from "@/lib/dashboard";
import { Markdown } from "@/components/Markdown";
import { Badge } from "@/components/ui/badge";
import {
  Sheet,
  SheetContent,
  SheetHeader,
} from "@/components/ui/sheet";

type SpecSidePanelProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  projectId: string | null;
  projectPath: string | null;
  specName: string | null;
  specRow: SpecRow | null;
};

export function SpecSidePanel({
  open,
  onOpenChange,
  projectId,
  projectPath,
  specName,
  specRow,
}: SpecSidePanelProps) {
  const { data: markdown, isLoading, error } = useQuery({
    queryKey: ["spec-markdown", projectPath, specName],
    queryFn: () => fetchSpecMarkdown(projectPath!, specName!),
    enabled: open && !!projectPath && !!specName,
    staleTime: 60_000,
  });

  // An empty or whitespace-only string is not "content" — treat it like a
  // missing spec so the panel shows an explanatory message, not a blank body.
  const hasContent = typeof markdown === "string" && markdown.trim().length > 0;

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent>
        <SheetHeader className="px-5 py-4 border-b">
          <div className="flex items-start gap-2 pr-8">
            <span className="font-mono text-sm font-medium truncate flex-1" title={specName ?? ""}>
              {specName ?? "—"}
            </span>
          </div>
          <div className="flex items-center gap-2 flex-wrap">
            {specRow?.phase && (
              <Badge variant="secondary" className="text-[11px] py-0">
                {specRow.phase}
              </Badge>
            )}
            {specRow?.status && (
              <Badge variant="outline" className="text-[11px] py-0">
                {specRow.status}
              </Badge>
            )}
            {projectId && specName && (
              <Link
                to={`/project/${projectId}/spec/${encodeURIComponent(specName)}`}
                onClick={() => onOpenChange(false)}
                className="ml-auto text-[12px] text-primary hover:underline underline-offset-2 whitespace-nowrap"
              >
                Abrir página completa →
              </Link>
            )}
          </div>
        </SheetHeader>

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
          {!isLoading && !error && !hasContent && specName && (
            <p className="text-[13px] text-muted-foreground">
              Esta spec não tem um <code className="font-mono">spec.md</code> com conteúdo.
            </p>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}
