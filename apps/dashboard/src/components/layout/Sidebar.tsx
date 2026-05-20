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
  FileText,
  FolderPlus,
  Cog,
  ChevronRight,
  ChevronDown,
  MoreHorizontal,
  Loader2,
} from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Separator } from "@/components/ui/separator";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { useStore } from "@/lib/store";
import {
  useProjectsStore,
  type ProjectEntry,
} from "@/lib/projects-store";
import { useProjectDetections } from "@/hooks/useProjectDetections";
import { useArtifactDrift } from "@/hooks/useArtifactDrift";
import {
  updateMustard,
  uninstallMustard,
  artifactUpdateApply,
  type ProjectDetection,
  type ArtifactDriftReport,
} from "@/lib/projects";
import { useIsMustardRepo } from "@/hooks/useArtifactDrift";

// ---------------------------------------------------------------------------
// Shared styling
// ---------------------------------------------------------------------------

const groupHeaderClass =
  "text-xs uppercase tracking-wider font-medium text-muted-foreground px-3 py-1.5";

const toolNavItemClass = ({ isActive }: { isActive: boolean }) =>
  `flex items-center gap-2 px-3 py-1.5 rounded-md text-sm transition-colors duration-150 ${
    isActive
      ? "bg-primary/10 text-primary font-medium"
      : "text-sidebar-foreground/70 hover:bg-muted/40 hover:text-foreground"
  }`;

// Per-project leaf links share styling with the tools group but indent under
// the parent project header. Active-state highlight follows NavLink semantics.
const leafItemClass = (active: boolean) =>
  cn(
    "flex items-center gap-2 pl-9 pr-3 py-1.5 rounded-md text-sm transition-colors duration-150",
    active
      ? "bg-primary/10 text-primary font-medium"
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
      ? "bg-[--color-ok] ring-[--color-ok]/30"
      : kind === "updateAvailable"
        ? "bg-[--color-accent-mustard] ring-[--color-accent-mustard]/30"
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
  const navigate = useNavigate();
  const location = useLocation();
  const queryClient = useQueryClient();
  const removeProject = useProjectsStore((s) => s.removeProject);
  const activateProject = useProjectsStore((s) => s.activateProject);
  const isMustardRepo = useIsMustardRepo(project.path);
  const [menuOpen, setMenuOpen] = useState(false);
  const [actionPending, setActionPending] = useState<
    "update" | "uninstall" | "artifacts" | null
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
  const showUpdate = updateAvailable;
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
  // sidebar.status.* keys for suffixes; only the "version unknown" branch
  // needs a dedicated phrase. Color is binary green/red per user directive
  // ("verde se ok e vermelho se não"); checking stays muted.
  let statusLine: string;
  let statusLineColor: string;
  if (kind === "checking") {
    statusLine = t("sidebar.status.checking");
    statusLineColor = "text-muted-foreground";
  } else if (!detection?.installed) {
    statusLine = t("sidebar.status.notInstalled");
    statusLineColor = "text-red-400";
  } else if (!detection.version) {
    statusLine = t("sidebar.status.versionUnknown");
    statusLineColor = "text-[--color-ok]";
  } else {
    const suffix = updateAvailable
      ? t("sidebar.status.updateAvailable")
      : t("sidebar.status.installed");
    statusLine = `v${detection.version} · ${suffix}`;
    statusLineColor = updateAvailable ? "text-red-400" : "text-[--color-ok]";
  }

  async function ensureActive() {
    if (!isActive) await activateProject(project.path);
  }

  async function runUpdate() {
    setActionPending("update");
    try {
      await updateMustard(project.path);
      toast.success(t("projects.toastUpdated", { name: project.name }));
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
  // path matches AND the project itself is the active workspace.
  const leaves: { to: string; icon: typeof Home; label: string; end?: boolean }[] = [
    { to: "/workspace", icon: Home, label: "Visão Geral" },
    { to: "/specs", icon: FileText, label: "Specs" },
    { to: "/economy", icon: Gauge, label: "Economia" },
    { to: "/knowledge", icon: BookOpen, label: "Knowledge" },
    { to: "/settings", icon: SettingsIcon, label: "Configurações" },
  ];

  return (
    <div className="flex flex-col">
      <div
        className={cn(
          "group/row flex items-center gap-1 pl-1 pr-1 py-1 rounded-md transition-colors",
          isActive
            ? "bg-muted/40"
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
                    "border border-[--color-accent-mustard]/30 bg-[--color-accent-mustard]/10 text-[--color-accent-mustard]",
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
            {showUpdate && (
              <DropdownMenuItem
                onSelect={() => {
                  setMenuOpen(false);
                  void runUpdate();
                }}
              >
                {t("sidebar.projectMenu.update")}
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
            {(showArtifactUpdate || showUpdate || showUninstall) && (
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
// Sidebar
// ---------------------------------------------------------------------------

export function Sidebar() {
  const { t } = useTranslation();
  const projects = useProjectsStore((s) => s.projects);
  const addProject = useProjectsStore((s) => s.addProject);
  const activeProjectsRoot = useStore((s) => s.projectsRoot);
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

  return (
    <aside className="row-span-2 col-start-1 bg-sidebar text-sidebar-foreground border-r border-sidebar-border flex flex-col">
      <div className="px-2 pt-3 flex flex-col flex-1 min-h-0">
        <button
          type="button"
          onClick={handleAddProject}
          disabled={!tauri || busyAdd}
          title={tauri ? undefined : t("sidebar.addProjectDesktopOnly")}
          className={cn(
            "flex items-center gap-2 px-3 py-1.5 rounded-md text-sm transition-colors duration-150",
            "text-sidebar-foreground/80 hover:bg-muted/40 hover:text-foreground",
            (!tauri || busyAdd) && "opacity-50 cursor-not-allowed",
          )}
        >
          {busyAdd ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <FolderPlus className="h-3.5 w-3.5" />
          )}
          <span>{t("sidebar.addProject")}</span>
        </button>

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

        <div className={groupHeaderClass}>{t("sidebar.tools")}</div>
        <NavLink to="/commands" className={toolNavItemClass}>
          <Terminal className="h-3.5 w-3.5" /> {t("nav.commands")}
        </NavLink>
        <NavLink to="/prd" className={toolNavItemClass}>
          <FileText className="h-3.5 w-3.5" /> {t("nav.prd")}
        </NavLink>

        <div className="mt-auto" />
        <Separator className="my-3" />
        <NavLink to="/preferences" className={toolNavItemClass}>
          <Cog className="h-3.5 w-3.5" /> {t("nav.preferences")}
        </NavLink>
      </div>
    </aside>
  );
}
