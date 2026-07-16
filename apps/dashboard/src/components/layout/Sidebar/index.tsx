// Sidebar — project tree navigation (B6 Wave 2).
//
// Replaces the previous flat workspace switcher + nav list with a project-
// scoped tree: each registered project is a collapsible node, and its leaves
// (Home/Activity/Telemetry/Quality/Knowledge/Settings) navigate into that
// project's context. Selecting a leaf also activates the project as the
// current workspace (`projectsRoot` + `activeWorkspaceId`) so the page-level
// queries fan out against the right folder.
//
// State model
// -----------
// - `projects` from `useProjectsStore` is the curated registry (slice select).
// - `projectsRoot` from `useStore` is the active workspace path; we expand
//   that node by default and keep manual toggles in local state for the rest.
// - Detection state per project comes from `useProjectDetections` (TanStack
//   `useQueries`, keyed by `project.path`); we map detection → status dot
//   colour + kebab-menu item visibility.
//
// Why no logo header: the existing layout (AppShell) reserves a 12px top row
// for Topbar only; the Sidebar starts flush. We preserve that — adding a
// logo block here would require coordinating with AppShell's grid rows.

import { useEffect, useState } from "react";
import { NavLink, useNavigate, useLocation } from "react-router";
import {
  Home,
  Settings as SettingsIcon,
  BookOpen,
  Gauge,
  Terminal,
  Activity as ActivityIcon,
  FolderPlus,
  ChevronRight,
  ChevronDown,
  MoreHorizontal,
  Loader2,
  PanelLeftClose,
  PanelLeftOpen,
  Box,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useT } from "@/lib/i18n";
import { Separator } from "@/components/ui/separator";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { useStore } from "@/lib/store";
import {
  useProjectsStore,
  type ProjectEntry,
} from "@/lib/projects-store";
import { useProjectDetections } from "@/hooks/useProjectDetections";
import { useArtifactDrift } from "@/hooks/useArtifactDrift";
import {
  uninstallMustard,
  artifactUpdateApply,
  type ProjectDetection,
  type ArtifactDriftReport,
} from "@/lib/projects";
import { useIsMustardRepo } from "@/hooks/useArtifactDrift";
import { DoctorBadge } from "@/components/DoctorBadge";

// ---------------------------------------------------------------------------
// Shared styling
// ---------------------------------------------------------------------------

const groupHeaderClass =
  "text-xs uppercase tracking-wider text-muted-foreground px-3 py-1.5";

const toolNavItemClass = ({ isActive }: { isActive: boolean }) =>
  `flex items-center gap-2 px-3 py-1.5 rounded-md text-sm transition-colors duration-150 ${
    isActive
      ? "bg-primary/15 text-foreground font-medium"
      : "text-sidebar-foreground/70 hover:bg-muted/40 hover:text-foreground"
  }`;

// Per-project leaf links share styling with the tools group but indent under
// the parent project header. Active-state highlight follows NavLink semantics.
const leafItemClass = (active: boolean) =>
  cn(
    "flex items-center gap-2 pl-9 pr-3 py-1.5 rounded-md text-sm transition-colors duration-150",
    active
      ? "bg-primary/15 text-foreground font-medium"
      : "text-sidebar-foreground/70 hover:bg-muted/40 hover:text-foreground",
  );

// ---------------------------------------------------------------------------
// Detection → status dot variant
// ---------------------------------------------------------------------------

type StatusKind = "installed" | "updateAvailable" | "notInstalled" | "checking";

function statusKind(
  isLoading: boolean,
  detection: ProjectDetection | undefined,
): StatusKind {
  if (isLoading || !detection) return "checking";
  if (!detection.installed) return "notInstalled";
  // Update detection is path-based; until a bundled-version constant exists
  // on the dashboard surface we mirror ProjectCard's deriveUpdateAvailable
  // (always false). The kebab "Update" item gates on this too.
  return "installed";
}

function StatusDotInline({ kind }: { kind: StatusKind }) {
  if (kind === "checking") {
    return (
      <Loader2 className="h-3 w-3 animate-spin text-muted-foreground shrink-0" />
    );
  }
  const color =
    kind === "installed"
      ? "bg-[--intent-success] ring-[--intent-success]/30"
      : kind === "updateAvailable"
        ? "bg-[--primary] ring-[--primary]/30"
        : "bg-zinc-500 ring-zinc-500/30";
  return (
    <span
      aria-hidden
      className={cn("w-2 h-2 rounded-full ring-1 shrink-0", color)}
    />
  );
}

// ---------------------------------------------------------------------------
// Tauri runtime detection (parity with deleted AddProjectButton)
// ---------------------------------------------------------------------------

function isTauriRuntime(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof (window as unknown as { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__ !== "undefined"
  );
}

// ---------------------------------------------------------------------------
// ProjectTreeNode — one row + collapsible leaves
// ---------------------------------------------------------------------------

interface ProjectTreeNodeProps {
  project: ProjectEntry;
  detection: ProjectDetection | undefined;
  isLoading: boolean;
  isActive: boolean;
  isExpanded: boolean;
  onToggleExpand: () => void;
  /** Artifact-drift report for this project (B6 Wave 3). `undefined` when the
   *  query is still loading or the probe failed (e.g. `mustard-rt` missing,
   *  manifest absent on consumer projects). */
  driftReport: ArtifactDriftReport | undefined;
}

function ProjectTreeNode({
  project,
  detection,
  isLoading,
  isActive,
  isExpanded,
  onToggleExpand,
  driftReport,
}: ProjectTreeNodeProps) {
  const { t } = useTranslation();
  // `tLib` is the W2-audit canonical surface from `@/lib/i18n`. We keep the
  // i18next `t` (above) for the project-card namespace (`projects.*`,
  // `sidebar.status.*`, `artifact.*`, etc.) and use `tLib` for the leaf nav
  // labels added by the i18n audit so "Knowledge" flips to "Conhecimento" in
  // PT mode without polluting the existing i18next dictionary.
  const tLib = useT();
  const navigate = useNavigate();
  const location = useLocation();
  const queryClient = useQueryClient();
  const removeProject = useProjectsStore((s) => s.removeProject);
  const activateProject = useProjectsStore((s) => s.activateProject);
  const isMustardRepo = useIsMustardRepo(project.path);
  const [menuOpen, setMenuOpen] = useState(false);
  const [actionPending, setActionPending] = useState<
    "uninstall" | "artifacts" | null
  >(null);

  const staleCount = driftReport?.stale ?? 0;
  const hasStale = staleCount > 0;
  // The "Update artifacts" action only makes sense in the canonical Mustard
  // repo — its `apps/cli/templates/` is the authoritative payload. On consumer
  // projects the manifest does not exist, so we hide the entry to avoid an
  // action that can only ever fail.
  const showArtifactUpdate = hasStale && isMustardRepo;

  const kind = statusKind(isLoading, detection);
  const updateAvailable = kind === "updateAvailable";
  const showUninstall = detection?.installed === true;

  const statusLabel =
    kind === "checking"
      ? t("sidebar.status.checking")
      : kind === "installed"
        ? t("sidebar.status.installed")
        : kind === "updateAvailable"
          ? t("sidebar.status.updateAvailable")
          : t("sidebar.status.notInstalled");

  // Discreet second line under the project name. Reuses the existing
  // sidebar.status.* keys. The version number is no longer shown here — it
  // moved to the Overview page (ProjectInfoCard); this line now carries only
  // the install state. Color is binary green/red per user directive
  // ("verde se ok e vermelho se não"); checking stays muted.
  let statusLine: string;
  let statusLineColor: string;
  if (kind === "checking") {
    statusLine = t("sidebar.status.checking");
    statusLineColor = "text-muted-foreground";
  } else if (!detection?.installed) {
    statusLine = t("sidebar.status.notInstalled");
    statusLineColor = "text-[--intent-error]";
  } else {
    statusLine = updateAvailable
      ? t("sidebar.status.updateAvailable")
      : t("sidebar.status.installed");
    statusLineColor = updateAvailable ? "text-[--intent-error]" : "text-[--intent-success]";
  }

  async function ensureActive() {
    if (!isActive) await activateProject(project.path);
  }

  async function runUninstall() {
    setActionPending("uninstall");
    try {
      await uninstallMustard(project.path);
      toast.success(t("projects.toastUninstalled", { name: project.name }));
      await queryClient.invalidateQueries({
        queryKey: ["project-detection", project.path],
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t("projects.toastActionFailed", { msg }));
    } finally {
      setActionPending(null);
    }
  }

  // Runs `mustard-rt run artifact-update --apply` for the canonical Mustard
  // repo. The subprocess rewrites `apps/cli/templates/` from upstream, so we
  // invalidate both the drift query (badge should drop to 0) and any consumer
  // surfaces that read the manifest. Toast text uses i18n keys; raw errors are
  // surfaced via `toastActionFailed`.
  async function runArtifactUpdate() {
    setActionPending("artifacts");
    const pendingToast = toast.loading(t("artifact.updateRunning"));
    try {
      const outcome = await artifactUpdateApply(project.path);
      toast.success(
        t("artifact.updateSuccess", { count: outcome.applied }),
        { id: pendingToast },
      );
      await queryClient.invalidateQueries({
        queryKey: ["artifact-drift", project.path],
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t("artifact.updateError", { msg }), { id: pendingToast });
    } finally {
      setActionPending(null);
    }
  }

  // Build leaves locally so the active-leaf style only fires when both the
  // path matches AND the project itself is the active workspace. Labels read
  // from `tLib` (`@/lib/i18n`) so PT/EN flips at runtime — see comment on
  // `tLib` above for why two `t` functions coexist in this component.
  const leaves: { to: string; icon: typeof Home; label: string; end?: boolean }[] = [
    { to: "/workspace", icon: Home, label: tLib("sidebar.overview") },
    { to: "/activity", icon: ActivityIcon, label: tLib("sidebar.activity") },
    { to: "/economy", icon: Gauge, label: tLib("sidebar.economy") },
    { to: "/knowledge", icon: BookOpen, label: tLib("sidebar.knowledge") },
    // Specs (/specs) and Sessões (/sessions) dropped from the per-project nav
    // (spec `dashboard-aba-atividade-redesenho`): both are drill-in destinations
    // reached FROM Activity, not standalone nav leaves. Their routes stay
    // registered in App.tsx.
    { to: "/settings", icon: SettingsIcon, label: tLib("sidebar.settings") },
  ];

  return (
    <div className="flex flex-col">
      <div
        className={cn(
          "group/row flex items-center gap-1 pl-1 pr-1 py-1 rounded-md transition-colors",
          isActive
            ? "bg-primary/15 ring-1 ring-inset ring-primary/40 border-l-2 border-primary"
            : "hover:bg-muted/30",
        )}
      >
        <button
          type="button"
          onClick={onToggleExpand}
          aria-label={isExpanded ? "Collapse" : "Expand"}
          aria-expanded={isExpanded}
          className="h-5 w-5 inline-flex items-center justify-center rounded text-muted-foreground hover:text-foreground shrink-0"
        >
          {isExpanded ? (
            <ChevronDown className="h-3.5 w-3.5" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5" />
          )}
        </button>
        <button
          type="button"
          onClick={async () => {
            await ensureActive();
            onToggleExpand();
          }}
          className="flex items-center gap-2 min-w-0 flex-1 text-left text-sm"
          title={project.path}
        >
          <StatusDotInline kind={kind} />
          <span className="flex flex-col min-w-0 flex-1">
            <span className="flex items-center gap-1.5 min-w-0">
              <span
                className={cn(
                  "truncate",
                  isActive
                    ? "text-foreground font-medium"
                    : "text-sidebar-foreground/80",
                )}
              >
                {project.name}
              </span>
              {hasStale && (
                <span
                  // Amber = "warning, not error" — matches the existing
                  // `updateAvailable` palette for the status dot. Tabular nums
                  // keep counts aligned across rows.
                  title={t("artifact.staleCount", { count: staleCount })}
                  className={cn(
                    "shrink-0 inline-flex items-center px-1.5 rounded text-[10px] leading-4 tabular-nums",
                    "border border-[--primary]/30 bg-[--primary]/10 text-[--primary]",
                  )}
                >
                  {t("artifact.staleCount", { count: staleCount })}
                </span>
              )}
            </span>
            <span className={cn("truncate text-[10px]", statusLineColor)}>
              {statusLine}
            </span>
          </span>
          <span className="sr-only">{statusLabel}</span>
        </button>
        <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label={`Open menu for ${project.name}`}
              className={cn(
                "h-6 w-6 inline-flex items-center justify-center rounded-md text-muted-foreground shrink-0",
                "opacity-0 group-hover/row:opacity-100 data-[state=open]:opacity-100",
                "hover:bg-muted hover:text-foreground transition-opacity",
                menuOpen && "opacity-100",
              )}
              data-state={menuOpen ? "open" : "closed"}
              disabled={actionPending !== null}
            >
              {actionPending !== null ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <MoreHorizontal className="h-3.5 w-3.5" />
              )}
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-44">
            {showArtifactUpdate && (
              <DropdownMenuItem
                onSelect={() => {
                  setMenuOpen(false);
                  void runArtifactUpdate();
                }}
              >
                {t("artifact.updateAction")}
              </DropdownMenuItem>
            )}
            {showUninstall && (
              <DropdownMenuItem
                onSelect={() => {
                  setMenuOpen(false);
                  void runUninstall();
                }}
              >
                {t("sidebar.projectMenu.uninstall")}
              </DropdownMenuItem>
            )}
            {(showArtifactUpdate || showUninstall) && (
              <DropdownMenuSeparator />
            )}
            <DropdownMenuItem
              variant="destructive"
              onSelect={() => {
                setMenuOpen(false);
                void removeProject(project.path);
              }}
            >
              {t("sidebar.projectMenu.removeFromRegistry")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {isExpanded && (
        <div className="flex flex-col gap-0.5 mt-0.5 mb-1">
          {leaves.map(({ to, icon: Icon, label, end }) => {
            const pathMatches = end
              ? location.pathname === to
              : location.pathname === to ||
                location.pathname.startsWith(`${to}/`);
            const active = isActive && pathMatches;
            return (
              <button
                key={to}
                type="button"
                onClick={async () => {
                  await ensureActive();
                  navigate(to);
                }}
                className={leafItemClass(active)}
              >
                <Icon className="h-3.5 w-3.5" />
                <span>{label}</span>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Collapsed icon rail
// ---------------------------------------------------------------------------

/** A single 56px-rail entry: an icon-only button with a hover tooltip. Used for
 *  both the tool nav and the per-project dots while the sidebar is collapsed. */
function RailButton({
  icon: Icon,
  label,
  active,
  disabled,
  spinning,
  onClick,
  dot,
}: {
  icon: typeof Home;
  label: string;
  active?: boolean;
  disabled?: boolean;
  spinning?: boolean;
  onClick: () => void;
  /** Optional status dot overlaid bottom-right (project install state). */
  dot?: StatusKind;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={onClick}
          disabled={disabled}
          aria-label={label}
          className={cn(
            "relative mx-auto inline-flex h-9 w-9 items-center justify-center rounded-md transition-colors duration-150",
            active
              ? "bg-primary/15 text-foreground"
              : "text-sidebar-foreground/70 hover:bg-muted/40 hover:text-foreground",
            disabled && "opacity-50 cursor-not-allowed",
          )}
        >
          <Icon className={cn("h-4 w-4", spinning && "animate-spin")} />
          {dot && dot !== "checking" && (
            <span
              aria-hidden
              className={cn(
                "absolute bottom-1 right-1 h-1.5 w-1.5 rounded-full ring-1 ring-background",
                dot === "installed"
                  ? "bg-[--intent-success]"
                  : dot === "updateAvailable"
                    ? "bg-[--primary]"
                    : "bg-zinc-500",
              )}
            />
          )}
        </button>
      </TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  );
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

export function Sidebar() {
  const { t } = useTranslation();
  // `tLib` powers W2-audit keys (`sidebar.add_project`, `sidebar.tools`,
  // `sidebar.commands`). The i18next
  // `t` still drives project-detection toasts, empty states, and the
  // `projects.addDialogTitle` Tauri dialog title (keys not duplicated here).
  const tLib = useT();
  const navigate = useNavigate();
  const location = useLocation();
  const projects = useProjectsStore((s) => s.projects);
  const addProject = useProjectsStore((s) => s.addProject);
  const activateProject = useProjectsStore((s) => s.activateProject);
  const activeProjectsRoot = useStore((s) => s.projectsRoot);
  const collapsed = useStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useStore((s) => s.toggleSidebar);
  const detections = useProjectDetections();
  const driftByPath = useArtifactDrift();
  const [busyAdd, setBusyAdd] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const tauri = isTauriRuntime();

  // Auto-expand the project whose path matches the active workspace root.
  // We only seed when the user has not manually toggled the node yet so a
  // user-initiated collapse is preserved across activations.
  useEffect(() => {
    if (!activeProjectsRoot) return;
    setExpanded((prev) =>
      prev[activeProjectsRoot] === undefined
        ? { ...prev, [activeProjectsRoot]: true }
        : prev,
    );
  }, [activeProjectsRoot]);

  async function handleAddProject() {
    if (!tauri || busyAdd) return;
    setBusyAdd(true);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t("projects.addDialogTitle"),
      });
      if (typeof selected !== "string" || !selected) return;
      await addProject(selected);
      // Adding activates the project (see projects-store), so seed it
      // expanded immediately rather than waiting for the effect tick.
      setExpanded((prev) => ({ ...prev, [selected]: true }));
    } finally {
      setBusyAdd(false);
    }
  }

  // Tools group, shared between the rail and the full tree so the icon set
  // stays in sync. `end` marks an exact-match active route.
  const tools: { to: string; icon: typeof Home; label: string }[] = [
    { to: "/commands", icon: Terminal, label: tLib("sidebar.commands") },
  ];

  // -------------------------------------------------------------------------
  // Collapsed icon rail (~56px): add-project, project dots, tools, prefs.
  // Projects collapse to a single status-dotted box icon that activates +
  // jumps to that project's overview (the full tree needs width it lacks here).
  // -------------------------------------------------------------------------
  if (collapsed) {
    return (
      <aside className="row-span-2 col-start-1 bg-background text-sidebar-foreground border-r border-border flex flex-col items-stretch">
        <TooltipProvider delayDuration={200}>
          <div className="flex flex-col items-stretch gap-1 px-2 pt-3 flex-1 min-h-0">
            <RailButton
              icon={PanelLeftOpen}
              label={tLib("sidebar.expand", "Expandir")}
              onClick={toggleSidebar}
            />
            <RailButton
              icon={busyAdd ? Loader2 : FolderPlus}
              spinning={busyAdd}
              disabled={!tauri || busyAdd}
              label={tLib("sidebar.add_project")}
              onClick={handleAddProject}
            />

            <Separator className="my-2" />

            <div className="flex flex-col gap-1 overflow-y-auto">
              {detections.map((row) => {
                const isActive = activeProjectsRoot === row.project.path;
                const kind = statusKind(row.isLoading, row.detection);
                return (
                  <RailButton
                    key={row.project.path}
                    icon={Box}
                    label={row.project.name}
                    active={isActive}
                    dot={kind}
                    onClick={async () => {
                      if (!isActive) await activateProject(row.project.path);
                      navigate("/workspace");
                    }}
                  />
                );
              })}
            </div>

            <Separator className="my-2" />

            {tools.map(({ to, icon, label }) => (
              <RailButton
                key={to}
                icon={icon}
                label={label}
                active={
                  location.pathname === to ||
                  location.pathname.startsWith(`${to}/`)
                }
                onClick={() => navigate(to)}
              />
            ))}

          </div>
        </TooltipProvider>
      </aside>
    );
  }

  return (
    <aside className="row-span-2 col-start-1 bg-background text-sidebar-foreground border-r border-border flex flex-col">
      <div className="px-2 pt-3 flex flex-col flex-1 min-h-0">
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={handleAddProject}
            disabled={!tauri || busyAdd}
            title={tauri ? undefined : t("sidebar.addProjectDesktopOnly")}
            className={cn(
              "flex flex-1 items-center gap-2 px-3 py-1.5 rounded-md text-sm transition-colors duration-150",
              "text-sidebar-foreground/80 hover:bg-muted/40 hover:text-foreground",
              (!tauri || busyAdd) && "opacity-50 cursor-not-allowed",
            )}
          >
            {busyAdd ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <FolderPlus className="h-3.5 w-3.5" />
            )}
            <span>{tLib("sidebar.add_project")}</span>
          </button>
          <button
            type="button"
            onClick={toggleSidebar}
            aria-label={tLib("sidebar.collapse", "Recolher")}
            title={tLib("sidebar.collapse", "Recolher")}
            className="h-7 w-7 shrink-0 inline-flex items-center justify-center rounded-md text-muted-foreground hover:bg-muted/40 hover:text-foreground transition-colors"
          >
            <PanelLeftClose className="h-3.5 w-3.5" />
          </button>
        </div>

        <div className="mt-2 flex flex-col gap-0.5 overflow-y-auto">
          {projects.length === 0 ? (
            <div className="px-3 py-4 text-center">
              <p className="text-sm font-medium text-foreground/80">
                {t("sidebar.empty.title")}
              </p>
              <p className="text-[12px] text-muted-foreground mt-1">
                {t("sidebar.empty.description")}
              </p>
            </div>
          ) : (
            detections.map((row) => {
              const isActive = activeProjectsRoot === row.project.path;
              const isExpanded = expanded[row.project.path] ?? false;
              return (
                <ProjectTreeNode
                  key={row.project.path}
                  project={row.project}
                  detection={row.detection}
                  isLoading={row.isLoading}
                  isActive={isActive}
                  isExpanded={isExpanded}
                  driftReport={driftByPath[row.project.path]?.report}
                  onToggleExpand={() =>
                    setExpanded((prev) => ({
                      ...prev,
                      [row.project.path]: !(prev[row.project.path] ?? false),
                    }))
                  }
                />
              );
            })
          )}
        </div>

        <Separator className="my-3" />

        <div className={groupHeaderClass}>{tLib("sidebar.tools")}</div>
        {tools.map(({ to, icon: Icon, label }) => (
          <NavLink key={to} to={to} className={toolNavItemClass}>
            <Icon className="h-3.5 w-3.5" /> {label}
          </NavLink>
        ))}

        <div className="mt-auto" />
        <Separator className="my-3" />
        {/* W10.T10.8 — installation health badge. Renders against the active
            workspace path; falls back to silent null when none is selected. */}
        <DoctorBadge projectPath={activeProjectsRoot ?? null} />
      </div>
    </aside>
  );
}
