import { useMemo, useState } from "react";
import {
  Boxes,
  Package,
  type LucideIcon,
  FileCode,
  Hexagon,
  ChevronRight,
  BookOpen,
  FileText,
  RefreshCw,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { cn } from "@/lib/utils";
import { DataCard, SectionHeader, StatPill, EmptyState } from "@/components/page";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { useProjectOverview } from "@/hooks/useProjectOverview";
import { useFileViewer } from "@/hooks/useFileViewer";
import {
  fetchDepsOutdated,
  type ProjectUnitSummary,
  type OutdatedDep,
  type DepSeverity,
} from "@/lib/dashboard";
import {
  TonalIcon,
  TONE,
  tonalStyle,
  type TonalColor,
} from "@/features/workspace/_shared/tonal";
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

/** Severity → tonal color. `up-to-date` reads success (green), `patch` info
 *  (blue), `minor` warning (amber), `major` error (red). The color is the
 *  signal, so a colour-blind-safe glyph is unnecessary here — text carries the
 *  `current → latest` detail beside it. */
const SEVERITY_COLOR: Record<DepSeverity, TonalColor> = {
  "up-to-date": TONE.success,
  patch: TONE.info,
  minor: TONE.warning,
  major: TONE.error,
};

/**
 * On-demand outdated check for one unit, gated behind an explicit click. The
 * `checked` flag (lifted by the parent node) flips `enabled` so the query never
 * fires on mount — the underlying Tauri command shells out to the registry and
 * is slow. Keyed by `[repoPath, dir]` so each project caches independently.
 * Fail-open: the command resolves to `[]` on any error, so `isError` is rare;
 * an empty result after a check is surfaced as a discreet "could not check" note
 * by the node, not an error toast.
 */
function useDepsOutdated(repoPath: string, unit: ProjectUnitSummary, checked: boolean) {
  return useQuery<OutdatedDep[]>({
    queryKey: ["deps-outdated", repoPath, unit.dir],
    queryFn: () => fetchDepsOutdated(repoPath, unit.dir, unit.language),
    enabled: checked && !!repoPath,
    staleTime: 5 * 60_000,
  });
}

/** A dep row with its merged outdated severity (when a check has run). */
interface DepView {
  name: string;
  version: string;
  outdated: OutdatedDep | null;
}

/** One project node: a collapsible treeview row. Header (name, dir, dep count,
 *  README/CLAUDE links, "check updates") is always shown; the dep list renders
 *  only when expanded. Default: collapsed. */
function UnitNode({
  unit,
  repoPath,
  openFile,
}: {
  unit: ProjectUnitSummary;
  repoPath: string;
  openFile: (relPath: string, fileName?: string) => void;
}) {
  const t = useT();
  const facet = langFacet(unit.language);
  const [expanded, setExpanded] = useState(false);
  const [checked, setChecked] = useState(false);

  const { data: outdated, isFetching } = useDepsOutdated(repoPath, unit, checked);

  // Merge the outdated result into the declared deps by name. A check that ran
  // but returned nothing (fail-open: no tool / no network / timeout) is flagged
  // so the node can show a discreet note instead of pretending all is current.
  const byName = useMemo(() => {
    const m = new Map<string, OutdatedDep>();
    for (const o of outdated ?? []) m.set(o.name, o);
    return m;
  }, [outdated]);

  const deps = useMemo<DepView[]>(
    () =>
      unit.deps.map((d) => ({
        name: d.name,
        version: d.version,
        outdated: byName.get(d.name) ?? null,
      })),
    [unit.deps, byName],
  );

  const extra = Math.max(0, unit.frameworks.length - MAX_FRAMEWORKS);
  // A check completed (not fetching) yet matched nothing — surface fail-open.
  const checkedEmpty = checked && !isFetching && (outdated?.length ?? 0) === 0;

  const onCheck = () => {
    setChecked(true);
    setExpanded(true);
  };

  return (
    <li className="rounded-md border border-border bg-card/30">
      <div className="flex items-start gap-2.5 px-2.5 py-2">
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
          aria-label={
            expanded
              ? t("overview.project.collapse", "colapsar projeto")
              : t("overview.project.expand", "expandir projeto")
          }
          className={cn(
            "mt-0.5 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded",
            "text-muted-foreground transition-colors hover:bg-muted/40",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
          )}
        >
          <ChevronRight
            className={cn(
              "h-3.5 w-3.5 transition-transform duration-150",
              expanded && "rotate-90",
            )}
            aria-hidden
          />
        </button>
        <TonalIcon icon={facet.icon} color={facet.color} />
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="flex min-w-0 flex-1 flex-col gap-1 text-left"
        >
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
        </button>
        <div className="flex shrink-0 items-center gap-1">
          <span className="font-mono text-[11px] tabular-nums text-muted-foreground">
            {t("overview.project.depsCount", "{n} libs").replace(
              "{n}",
              String(unit.deps.length),
            )}
          </span>
          {unit.readme_path && (
            <NodeAction
              icon={BookOpen}
              label="README"
              title={t("overview.project.openReadme", "abrir README")}
              onClick={() => openFile(unit.readme_path as string)}
            />
          )}
          {unit.claude_md_path && (
            <NodeAction
              icon={FileText}
              label="CLAUDE.md"
              title={t("overview.project.openClaudeMd", "abrir CLAUDE.md")}
              onClick={() => openFile(unit.claude_md_path as string)}
            />
          )}
          <NodeAction
            icon={RefreshCw}
            title={t("overview.project.checkUpdates", "checar atualizações")}
            onClick={onCheck}
            spinning={isFetching}
            disabled={isFetching}
          />
        </div>
      </div>

      {expanded && (
        <div className="border-t border-border/40 px-2.5 py-2">
          {unit.frameworks.length > 0 && (
            <div className="mb-2 flex flex-wrap gap-1">
              {unit.frameworks.slice(0, MAX_FRAMEWORKS).map((fw) => (
                <StatPill key={fw} value={fw} />
              ))}
              {extra > 0 && <StatPill value={`+${extra}`} />}
            </div>
          )}

          {isFetching && (
            <p className="mb-1.5 text-[11px] text-muted-foreground/80">
              {t("overview.project.checking", "checando atualizações…")}
            </p>
          )}
          {checkedEmpty && (
            <p className="mb-1.5 text-[11px] text-muted-foreground/70">
              {t(
                "overview.project.checkUnavailable",
                "não foi possível checar atualizações",
              )}
            </p>
          )}

          {deps.length === 0 ? (
            <p className="text-[11.5px] text-muted-foreground/70">
              {t("overview.project.noDeps", "sem dependências declaradas")}
            </p>
          ) : (
            <ul className="flex flex-col gap-0.5">
              {deps.map((dep) => (
                <DepRow key={dep.name} dep={dep} />
              ))}
            </ul>
          )}
        </div>
      )}
    </li>
  );
}

/** A discreet icon button for the node header (README / CLAUDE.md / check). */
function NodeAction({
  icon: Icon,
  label,
  title,
  onClick,
  spinning,
  disabled,
}: {
  icon: LucideIcon;
  label?: string;
  title: string;
  onClick: () => void;
  spinning?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={title}
      className={cn(
        "inline-flex items-center gap-1 rounded px-1.5 py-1 text-[11px]",
        "text-muted-foreground transition-colors hover:bg-muted/40 hover:text-foreground",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
        "disabled:opacity-50",
      )}
    >
      <Icon className={cn("h-3.5 w-3.5", spinning && "animate-spin")} aria-hidden />
      {label && <span className="hidden sm:inline">{label}</span>}
    </button>
  );
}

/** One dependency row: name + installed version (mono), plus a severity dot and
 *  `current → latest` when a check ran and flagged it. Libs absent from the
 *  outdated result (no check, or current) read neutral grey. */
function DepRow({ dep }: { dep: DepView }) {
  const o = dep.outdated;
  const severity = o?.severity;
  // Only render a coloured indicator for known severities; an absent or
  // `up-to-date` dep stays neutral (no false "stale" signal).
  const color =
    severity && severity !== "up-to-date" ? SEVERITY_COLOR[severity] : null;

  return (
    <li className="flex items-center gap-2 py-0.5 text-[11.5px]">
      <span
        aria-hidden
        className="h-1.5 w-1.5 shrink-0 rounded-full"
        style={
          color
            ? tonalStyle(color)
            : { backgroundColor: "var(--muted-foreground)", opacity: 0.4 }
        }
      />
      <span className="truncate text-foreground/80">{dep.name}</span>
      <span className="shrink-0 font-mono text-[10.5px] text-muted-foreground tabular-nums">
        {dep.version || "—"}
      </span>
      {color && o && (
        <span
          className="shrink-0 font-mono text-[10.5px] tabular-nums"
          style={{ color }}
          title={`${o.severity}: ${o.current} → ${o.latest}`}
        >
          {o.current} → {o.latest}
        </span>
      )}
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
 * the fold. Inside each tab, each project is a collapsible treeview node
 * (default collapsed): the header shows name, directory, dep count, README /
 * CLAUDE.md links and a "check updates" action; expanding reveals the unit's
 * frameworks and its dependency list (name + installed version). The outdated
 * check is on-demand — clicking "check updates" fires `dashboard_deps_outdated`
 * and paints each stale dep by semver severity. The initial tab is the one with
 * the most projects. Empty-state tolerant — an unscanned workspace resolves to
 * an empty overview (the Tauri command is fail-open).
 */
export function ProjectInfoCard({ repoPath }: ProjectInfoCardProps) {
  const t = useT();
  const { data } = useProjectOverview(repoPath);
  const { openFile, viewer } = useFileViewer(repoPath);

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
              <ul className="flex flex-col gap-1.5">
                {g.units.map((unit) => (
                  <UnitNode
                    key={`${unit.dir}:${unit.name}`}
                    unit={unit}
                    repoPath={repoPath}
                    openFile={openFile}
                  />
                ))}
              </ul>
            </TabsContent>
          ))}
        </Tabs>
      )}

      {viewer}
    </DataCard>
  );
}
