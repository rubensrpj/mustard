import { useState } from "react";
import { Boxes, GitBranch, Flame, type LucideIcon } from "lucide-react";
import { ProjectInfoCard } from "@/features/workspace/ProjectInfoCard";
import { GitInfoCard } from "@/features/workspace/GitInfoCard";
import { WorkspaceFilesRanking } from "@/features/workspace/WorkspaceFilesRanking";
import { TonalIcon, type TonalColor } from "@/features/workspace/_shared/tonal";
import { cn } from "@/lib/utils";

/**
 * Tabbed viewer for the "Projetos" section of the overview. The three project
 * viewers (identity, local git, most-touched files) used to stack full-width,
 * which forced a long scroll; a left sub-sidebar replaced that but wasted the
 * horizontal space. Here they become a horizontal pill-tab strip at the top and
 * a single full-width content panel below — the active viewer fills the whole
 * section, no dead lateral column.
 *
 * The strip is a segmented control: each pill carries its viewer's tonal icon +
 * label, the active one tinted in its own hue (the colour carries the meaning;
 * grey is structure), inactives muted with a discreet hover. Each viewer keeps
 * the `repoPath`-only contract it already exposes.
 */
type PanelKey = "projeto" | "git" | "arquivos";

interface PanelItem {
  key: PanelKey;
  label: string;
  icon: LucideIcon;
  /** Item hue — `var(--…)` token or brand hex. Drives the tonal icon, the
   *  active pill's tint and label, all via `color-mix` (never a Tailwind
   *  opacity modifier over a hex var — that yields invalid CSS). */
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
    <div className="flex flex-col gap-4">
      {/* Pill-tab strip — a horizontal segmented control on its own canvas row. */}
      <nav
        aria-label="Visualizadores do projeto"
        className="flex flex-wrap gap-1 self-start rounded-lg border border-border bg-background p-1.5"
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
              // Active pill: tint + label in the item's hue. Inactive pills stay
              // muted (structure), with a discreet hover. Tint uses inline
              // `color-mix` per `tonal.tsx`.
              className={cn(
                "flex items-center gap-2 rounded-md px-3 py-1.5 text-left text-sm transition-colors duration-150",
                "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
                isActive
                  ? "font-medium"
                  : "text-muted-foreground hover:bg-muted/40 hover:text-foreground",
              )}
              style={
                isActive
                  ? {
                      color,
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

      {/* Content panel — only the active viewer renders, one at a time, full width. */}
      <div className="min-w-0">
        {active === "projeto" && <ProjectInfoCard repoPath={repoPath} />}
        {active === "git" && <GitInfoCard repoPath={repoPath} />}
        {active === "arquivos" && <WorkspaceFilesRanking repoPath={repoPath} />}
      </div>
    </div>
  );
}
