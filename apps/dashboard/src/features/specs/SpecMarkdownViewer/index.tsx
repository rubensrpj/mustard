import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { FileText, AlertCircle } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { Markdown } from "@/components/page/Markdown";
import { fetchSpecMarkdown } from "@/lib/dashboard";
import { cn } from "@/lib/utils";

/**
 * SpecMarkdownViewer — full-screen modal that renders the spec's markdown
 * artifacts. The Tauri command `dashboard_spec_markdown` resolves a single
 * markdown blob from a directory name; this viewer exposes the four kinds
 * the user can navigate to:
 *
 *   - "spec"   → `{spec}/spec.md` (or wave-plan.md fallback handled by Rust)
 *   - "wave"   → `{spec}/wave-{N}*` child resolved by name match (Rust case 2)
 *   - "qa"     → `{spec}/qa-report` or `{spec}/qa` child
 *   - "review" → `{spec}/review-report` or `{spec}/review` child
 *
 * For "wave"/"qa"/"review" the actual on-disk layout varies — we attempt the
 * most common names in order and surface a clear "indisponível" state when
 * none match. The Rust traversal guard means each candidate must be a bare
 * directory name (no slashes).
 */
export type SpecMarkdownKind = "spec" | "wave" | "qa" | "review";

interface SpecMarkdownViewerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  repoPath: string | null;
  spec: string;
  /** Optional list of wave numbers to show as wave tabs. */
  waves?: number[];
  initialKind?: SpecMarkdownKind;
  initialWave?: number;
}

interface TabSpec {
  id: string;
  label: string;
  kind: SpecMarkdownKind;
  wave?: number;
  /** Candidate directory names to try in order against dashboard_spec_markdown. */
  candidates: string[];
}

function buildTabs(spec: string, waves: number[]): TabSpec[] {
  const tabs: TabSpec[] = [
    { id: "spec", label: "Spec", kind: "spec", candidates: [spec] },
  ];
  for (const n of waves) {
    tabs.push({
      id: `wave-${n}`,
      label: `Onda ${n}`,
      kind: "wave",
      wave: n,
      // Rust case 2 searches every parent dir for a child whose name matches.
      // We try the most common naming conventions: bare `wave-N` and a few
      // typical suffixes the pipeline uses. Stop at the first hit.
      candidates: [`wave-${n}`, `wave-${n}-impl`, `wave-${n}-frontend`, `wave-${n}-backend`],
    });
  }
  tabs.push(
    { id: "qa", label: "QA", kind: "qa", candidates: ["qa-report", "qa"] },
    { id: "review", label: "Review", kind: "review", candidates: ["review-report", "review"] },
  );
  return tabs;
}

async function resolveMarkdown(
  repoPath: string,
  candidates: string[],
): Promise<string> {
  let lastErr = "";
  for (const name of candidates) {
    try {
      const md = await fetchSpecMarkdown(repoPath, name);
      if (md && md.length > 0) return md;
    } catch (e) {
      lastErr = e instanceof Error ? e.message : String(e);
    }
  }
  throw new Error(lastErr || "markdown não disponível");
}

function NotAvailable({ kind }: { kind: SpecMarkdownKind }) {
  const labels: Record<SpecMarkdownKind, string> = {
    spec: "O arquivo spec.md ainda não foi gerado para esta spec.",
    wave: "Esta onda não tem markdown próprio.",
    qa: "Nenhum qa-report foi gerado ainda.",
    review: "Nenhum review-report foi gerado ainda.",
  };
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-10 text-muted-foreground">
      <AlertCircle className="h-5 w-5 text-muted-foreground/70" aria-hidden />
      <p className="text-[13px]">{labels[kind]}</p>
    </div>
  );
}

export function SpecMarkdownViewer({
  open,
  onOpenChange,
  repoPath,
  spec,
  waves = [],
  initialKind = "spec",
  initialWave,
}: SpecMarkdownViewerProps) {
  const tabs = useMemo(() => buildTabs(spec, waves), [spec, waves]);

  const initialId = useMemo(() => {
    if (initialKind === "wave" && initialWave != null) {
      return `wave-${initialWave}`;
    }
    if (initialKind === "qa") return "qa";
    if (initialKind === "review") return "review";
    return "spec";
  }, [initialKind, initialWave]);

  const [activeId, setActiveId] = useState<string>(initialId);

  // Reset when the spec or initial selection changes (e.g. opening from
  // a different drill-down row). Following the dashboard convention of
  // resetting internal state on query-key changes.
  useEffect(() => {
    if (open) setActiveId(initialId);
  }, [open, initialId, spec]);

  const activeTab = tabs.find((t) => t.id === activeId) ?? tabs[0];

  const mdQuery = useQuery({
    queryKey: ["spec-md", repoPath, spec, activeTab.id],
    queryFn: () => resolveMarkdown(repoPath as string, activeTab.candidates),
    enabled: !!repoPath && open,
    staleTime: 10_000,
    retry: false,
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        // Full-screen takeover — override the small default max-width.
        className="!max-w-[min(96vw,1100px)] !w-[min(96vw,1100px)] h-[90vh] grid-rows-[auto_auto_1fr] p-0 gap-0 overflow-hidden"
      >
        <div className="flex items-center gap-2 px-4 pt-4 pb-2">
          <FileText className="h-4 w-4 text-muted-foreground" aria-hidden />
          <DialogTitle className="font-mono text-[13px] truncate">{spec}</DialogTitle>
        </div>
        <DialogDescription className="sr-only">
          Markdown viewer for spec {spec}
        </DialogDescription>

        {/* Tab strip */}
        <div
          role="tablist"
          aria-label="Seções do markdown"
          className="flex items-center gap-1 px-3 border-b border-border/60 overflow-x-auto"
        >
          {tabs.map((t) => {
            const selected = t.id === activeId;
            return (
              <button
                key={t.id}
                role="tab"
                aria-selected={selected}
                onClick={() => setActiveId(t.id)}
                className={cn(
                  "shrink-0 text-[12px] px-2.5 py-1.5 rounded-t-md transition-colors",
                  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
                  selected
                    ? "text-foreground border-b-2 border-[--primary] -mb-px font-medium"
                    : "text-muted-foreground hover:text-foreground hover:bg-muted/40",
                )}
              >
                {t.label}
              </button>
            );
          })}
        </div>

        {/* Body */}
        <div className="overflow-auto px-5 py-4 min-h-0">
          {mdQuery.isLoading ? (
            <ul className="flex flex-col gap-2">
              {[0, 1, 2, 3].map((i) => (
                <li
                  key={i}
                  className="h-4 rounded bg-muted/40 animate-pulse"
                  style={{ width: `${60 + ((i * 17) % 30)}%` }}
                />
              ))}
            </ul>
          ) : mdQuery.error ? (
            <NotAvailable kind={activeTab.kind} />
          ) : mdQuery.data ? (
            <Markdown content={mdQuery.data} />
          ) : (
            <NotAvailable kind={activeTab.kind} />
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
