import { useState } from "react";
import { Boxes, GitBranch, Flame, type LucideIcon } from "lucide-react";
import { ProjectInfoCard } from "@/features/workspace/ProjectInfoCard";
import { GitInfoCard } from "@/features/workspace/GitInfoCard";
import { WorkspaceFilesRanking } from "@/features/workspace/WorkspaceFilesRanking";
import { TonalIcon, type TonalColor } from "@/features/workspace/_shared/tonal";
import { cn } from "@/lib/utils";

/**
 * Master-detail for the "Projetos" section of the overview. The three project
 * viewers (identity, local git, most-touched files) used to stack full-width,
 * which forced a long scroll; here they become a left sub-sidebar of clickable
 * items and a right content panel that renders only the active one.
 *
 * The sub-sidebar mirrors the main `Sidebar` visual language — same canvas
 * background + a subtle border, the active row tinted in its item's own hue so
 * the colour carries the meaning (grey is structure). Each viewer keeps the
 * `repoPath`-only contract it already exposes.
 */
type PanelKey = "projeto" | "git" | "arquivos";

interface PanelItem {
  key: PanelKey;
  label: string;
  icon: LucideIcon;
  /** Item hue — `var(--…)` token or brand hex. Drives the tonal icon, the
   *  active row's left rail, tint and label, all via `color-mix` (never a
   *  Tailwind opacity modifier over a hex var — that yields invalid CSS). */
  color: TonalColor;
}

// Each item's colour matches the panel it opens: project identity keeps the
// neutral `--accent` (same as ProjectInfoCard's header), git the .NET brand
// violet, the file ranking the info blue.
const ITEMS: PanelItem[] = [
  { key: "projeto", label: "Projeto", icon: Boxes, color: "var(--accent-foreground)" },
  { key: "git", label: "Git", icon: GitBranch, color: "#512bd4" },
  { key: "arquivos", label: "Mais tocados", icon: Flame, color: "var(--intent-info)" },
];

export function ProjectsPanel({ repoPath }: { repoPath: string }) {
  const [active, setActive] = useState<PanelKey>("projeto");

  return (
    <div className="grid grid-cols-[190px_1fr] gap-4">
      {/* Sub-sidebar — same canvas bg + subtle border as the main Sidebar. */}
      <nav
        aria-label="Visualizadores do projeto"
        className="flex flex-col gap-0.5 self-start rounded-lg border border-border bg-background p-1.5"
      >
        {ITEMS.map(({ key, label, icon, color }) => {
          const isActive = active === key;
          return (
            <button
              key={key}
              type="button"
              onClick={() => setActive(key)}
              aria-pressed={isActive}
              aria-current={isActive ? "true" : undefined}
              // Active row: a left rail + tint in the item's hue, label in that
              // colour. Inactive rows stay muted (structure), with a discreet
              // hover. Tint/border use inline `color-mix` per `tonal.tsx`.
              className={cn(
                "flex items-center gap-2.5 rounded-md border-l-2 px-2.5 py-2 text-left text-sm transition-colors duration-150",
                isActive
                  ? "font-medium"
                  : "border-transparent text-muted-foreground hover:bg-muted/40 hover:text-foreground",
              )}
              style={
                isActive
                  ? {
                      color,
                      borderLeftColor: color,
                      backgroundColor: `color-mix(in srgb, ${color} 10%, transparent)`,
                    }
                  : undefined
              }
            >
              <TonalIcon icon={icon} color={color} />
              <span className="truncate">{label}</span>
            </button>
          );
        })}
      </nav>

      {/* Content panel — only the active viewer renders, one at a time. */}
      <div className="min-w-0">
        {active === "projeto" && <ProjectInfoCard repoPath={repoPath} />}
        {active === "git" && <GitInfoCard repoPath={repoPath} />}
        {active === "arquivos" && <WorkspaceFilesRanking repoPath={repoPath} />}
      </div>
    </div>
  );
}
