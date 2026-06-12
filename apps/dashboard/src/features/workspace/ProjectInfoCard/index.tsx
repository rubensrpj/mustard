import { useMemo } from "react";
import {
  Boxes,
  Package,
  type LucideIcon,
  FileCode,
  Hexagon,
} from "lucide-react";
import { DataCard, SectionHeader, StatPill, EmptyState } from "@/components/page";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useProjectOverview } from "@/hooks/useProjectOverview";
import type { ProjectUnitSummary } from "@/lib/dashboard";
import { TonalIcon, TONE, type TonalColor } from "@/features/workspace/_shared/tonal";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

interface ProjectInfoCardProps {
  repoPath: string;
}

/** How many frameworks render per project before the rest collapse to "+N". */
const MAX_FRAMEWORKS = 6;

interface LangFacet {
  label: string;
  icon: LucideIcon;
  /** Tonal color (a `var(--…)` token or a brand hex) — color carries meaning,
   *  each ecosystem keeps a stable hue so the eye groups projects by language. */
  color: TonalColor;
}

/**
 * Map a project `kind` (the model's only per-unit language signal — `cargo`,
 * `npm`, `dotnet`, …) onto a friendly language label + icon + semantic color so
 * the card answers "which language?" at a glance. Unknown kinds fall back to
 * the raw kind (capitalised) with the neutral accent hue rather than inventing
 * a label or a new palette. `.NET / C#` keeps the brand violet (`#512bd4`), Go
 * brand cyan (`#00add8`), Node/TS the info blue, Rust the warning orange,
 * Python the warning amber.
 */
const KIND_FACETS: Record<string, LangFacet> = {
  cargo: { label: "Rust", icon: FileCode, color: TONE.warning },
  npm: { label: "Node/TS", icon: Hexagon, color: TONE.info },
  pnpm: { label: "Node/TS", icon: Hexagon, color: TONE.info },
  yarn: { label: "Node/TS", icon: Hexagon, color: TONE.info },
  go: { label: "Go", icon: FileCode, color: "#00add8" },
  pip: { label: "Python", icon: FileCode, color: TONE.warning },
  poetry: { label: "Python", icon: FileCode, color: TONE.warning },
  uv: { label: "Python", icon: FileCode, color: TONE.warning },
  maven: { label: "Java", icon: FileCode, color: TONE.error },
  gradle: { label: "Java/Kotlin", icon: FileCode, color: TONE.error },
  composer: { label: "PHP", icon: FileCode, color: TONE.accent },
  bundler: { label: "Ruby", icon: FileCode, color: TONE.error },
  pub: { label: "Dart/Flutter", icon: FileCode, color: TONE.info },
  dotnet: { label: ".NET / C#", icon: FileCode, color: "#512bd4" },
  swift: { label: "Swift", icon: FileCode, color: TONE.warning },
};

function langFacet(kind: string): LangFacet {
  const known = KIND_FACETS[kind.toLowerCase()];
  if (known) return known;
  return {
    label: kind.charAt(0).toUpperCase() + kind.slice(1),
    icon: Package,
    color: TONE.accent,
  };
}

/** One project row inside a language tab. */
function UnitRow({ unit }: { unit: ProjectUnitSummary }) {
  const facet = langFacet(unit.language);
  const extra = Math.max(0, unit.frameworks.length - MAX_FRAMEWORKS);
  return (
    <li className="flex items-start gap-2.5 rounded-md border border-border bg-card/30 px-2.5 py-2">
      <TonalIcon icon={facet.icon} color={facet.color} />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate text-[13px] font-medium text-foreground">
            {unit.name}
          </span>
          <span
            className="shrink-0 text-[11px] font-medium"
            style={{ color: facet.color }}
          >
            {facet.label}
          </span>
        </div>
        <span
          className="truncate font-mono text-[11px] text-muted-foreground"
          title={unit.dir}
        >
          {unit.dir || "."}
        </span>
        {unit.frameworks.length > 0 && (
          <div className="mt-0.5 flex flex-wrap gap-1">
            {unit.frameworks.slice(0, MAX_FRAMEWORKS).map((fw) => (
              <StatPill key={fw} value={fw} />
            ))}
            {extra > 0 && <StatPill value={`+${extra}`} />}
          </div>
        )}
        {unit.stacks.length > 0 && (
          <ul className="mt-0.5 flex flex-wrap gap-x-3 gap-y-0.5">
            {unit.stacks.map((stack) => (
              <li
                key={stack.name}
                className="flex items-center gap-1 text-[11.5px] text-foreground/70"
              >
                <span className="font-mono">{stack.name}</span>
                <span className="text-[10.5px] text-muted-foreground tabular-nums">
                  {Math.round(stack.confidence * 100)}%
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </li>
  );
}

/** One language group: a friendly label, its kind, color, and the units. */
interface LangGroup {
  kind: string;
  facet: LangFacet;
  units: ProjectUnitSummary[];
}

/**
 * Project identity card for the workspace overview. Header: monorepo flag +
 * total project count. Body: language tabs (one per distinct `kind` — ".NET /
 * C#", "Node/TS", …) so a 12-unit monorepo never hides its .NET projects below
 * the fold. Each tab lists that language's projects — name, directory
 * (mono/muted), frameworks (badges, capped + "+N") and that project's stacks
 * with confidence. The initial tab is the one with the most projects.
 * Empty-state tolerant — an unscanned workspace resolves to an empty overview
 * (the Tauri command is fail-open).
 */
export function ProjectInfoCard({ repoPath }: ProjectInfoCardProps) {
  const t = useT();
  const { data } = useProjectOverview(repoPath);

  const units = useMemo<ProjectUnitSummary[]>(() => data?.units ?? [], [data?.units]);

  // Group units by language kind, preserving first-seen order; the default tab
  // is the group with the most projects (ties broken by first appearance).
  const groups = useMemo<LangGroup[]>(() => {
    const byKind = new Map<string, LangGroup>();
    for (const unit of units) {
      const kind = (unit.language || "outros").toLowerCase();
      let g = byKind.get(kind);
      if (!g) {
        g = { kind, facet: langFacet(kind), units: [] };
        byKind.set(kind, g);
      }
      g.units.push(unit);
    }
    return [...byKind.values()];
  }, [units]);

  const defaultTab = useMemo<string>(() => {
    if (groups.length === 0) return "";
    return groups.reduce((best, g) => (g.units.length > best.units.length ? g : best))
      .kind;
  }, [groups]);

  const hasData = !!data && data.project_count > 0;

  return (
    <DataCard padded>
      <SectionHeader
        title={t("overview.project.title", "Projeto")}
        right={
          hasData ? (
            <span className="inline-flex items-center gap-1.5 text-[11px] text-muted-foreground">
              <Boxes className="h-3.5 w-3.5 text-[--accent]" aria-hidden />
              {data.is_monorepo
                ? t("overview.project.monorepo", "monorepo")
                : t("overview.project.single", "projeto único")}
              {" · "}
              <span className="tabular-nums">{data.project_count}</span>
            </span>
          ) : undefined
        }
      />

      {!hasData || groups.length === 0 ? (
        <EmptyState
          className="mt-3"
          title={t("overview.project.empty.title", "Sem modelo do projeto")}
          description={t(
            "overview.project.empty.description",
            "Rode /mustard:scan para minerar linguagens e stacks deste workspace.",
          )}
        />
      ) : (
        <Tabs defaultValue={defaultTab} className="mt-3 gap-3">
          <TabsList variant="line" className="flex-wrap">
            {groups.map((g) => (
              <TabsTrigger key={g.kind} value={g.kind} className="gap-1.5">
                <span style={{ color: g.facet.color }}>{g.facet.label}</span>
                <span
                  className="inline-flex h-4 min-w-4 items-center justify-center rounded-full px-1 font-mono text-[10px] tabular-nums"
                  style={{
                    color: g.facet.color,
                    backgroundColor: `color-mix(in srgb, ${g.facet.color} 16%, transparent)`,
                  }}
                >
                  {g.units.length}
                </span>
              </TabsTrigger>
            ))}
          </TabsList>
          {groups.map((g) => (
            <TabsContent key={g.kind} value={g.kind}>
              <ul
                className={cn(
                  "flex flex-col gap-1.5",
                  g.units.length > 6 && "max-h-[360px] overflow-y-auto pr-1",
                )}
              >
                {g.units.map((unit) => (
                  <UnitRow key={`${unit.dir}:${unit.name}`} unit={unit} />
                ))}
              </ul>
            </TabsContent>
          ))}
        </Tabs>
      )}
    </DataCard>
  );
}
