// Project-registry command wrappers (B6 Wave 1).
//
// Single import site for the dashboard's per-project install/detection
// surface. Components MUST NOT reach the transport directly — they import
// from here.

import { call } from "@/lib/api-client";

export interface ProjectDetection {
  /** True when the project-root `<path>/mustard.json` exists (the workspace
   *  anchor every install writes — `.claude/CLAUDE.md` is no longer planted). */
  installed: boolean;
  /** Mustard CLI version stamped into `<path>/mustard.json`, when
   *  readable. `null` when the file is missing or malformed. */
  version: string | null;
}

export function detectProjectMustard(path: string): Promise<ProjectDetection> {
  return call<ProjectDetection>("detect_project_mustard", { path });
}

export function uninstallMustard(path: string): Promise<void> {
  return call<void>("uninstall_mustard", { path });
}

// ---------------------------------------------------------------------------
// Artifact-drift surface (B6 Wave 3).
//
// `mustard-rt run artifact-update --check` reads
// `apps/cli/templates/.artifacts.json` and reports one row per vendored
// artifact. The dashboard fans this out per project (TanStack `useQueries`)
// and renders a discrete badge when `stale > 0`. The `--apply` companion is
// only meaningful inside the canonical Mustard repo (its `templates/` is the
// authoritative payload) — the sidebar gates the action behind
// `isMustardRepo`.
// ---------------------------------------------------------------------------

export interface ArtifactDrift {
  artifactId: string;
  category: string;
  status: "up-to-date" | "stale" | "unknown" | "tracked" | string;
  sourceKind: string;
  localVersion: string | null;
  upstreamVersion: string | null;
}

export interface ArtifactDriftReport {
  total: number;
  stale: number;
  items: ArtifactDrift[];
}

export interface ArtifactUpdateOutcome {
  applied: number;
  manifestWritten: boolean;
}

// The backend returns snake_case field names (see `#[serde(rename_all =
// "snake_case")]` on the Rust structs). Map them once at the wrapper layer so
// the rest of the UI consumes the camelCase shapes declared above.
interface RawArtifactDrift {
  artifact_id: string;
  category: string;
  status: string;
  source_kind: string;
  local_version: string | null;
  upstream_version: string | null;
}

interface RawArtifactDriftReport {
  total: number;
  stale: number;
  items: RawArtifactDrift[];
}

interface RawArtifactUpdateOutcome {
  applied: number;
  manifest_written: boolean;
}

export async function artifactUpdateCheck(
  projectPath: string,
): Promise<ArtifactDriftReport> {
  const raw = await call<RawArtifactDriftReport>("artifact_update_check", {
    projectPath,
  });
  return {
    total: raw.total,
    stale: raw.stale,
    items: raw.items.map((i) => ({
      artifactId: i.artifact_id,
      category: i.category,
      status: i.status,
      sourceKind: i.source_kind,
      localVersion: i.local_version,
      upstreamVersion: i.upstream_version,
    })),
  };
}

export async function artifactUpdateApply(
  projectPath: string,
): Promise<ArtifactUpdateOutcome> {
  const raw = await call<RawArtifactUpdateOutcome>("artifact_update_apply", {
    projectPath,
  });
  return { applied: raw.applied, manifestWritten: raw.manifest_written };
}

export function isMustardRepo(projectPath: string): Promise<boolean> {
  return call<boolean>("is_mustard_repo", { projectPath });
}

// ---------------------------------------------------------------------------
// The machine-level project registry.
//
// The list of folders the dashboard tracks is server state, kept under
// `~/.claude/`. The dashboard covers EVERY Mustard project on the machine, so
// the list is a fact ABOUT the machine: held in browser storage instead,
// opening the dashboard from a second browser — or from a phone over the
// network — would show an empty list, and neither view would be the truth.
//
// Registration is distinct from DISCOVERY (`@/api/discovery`, which walks the
// disk looking for `mustard.json`): the registry records a choice, the scan
// finds candidates.
//
// Taking a folder off the list is `hideProject`, not `unregisterProject`: the
// automatic writers (`mustard init`, the session observer, the scan) would put
// a dropped row straight back, so removal is a MARK the row carries and every
// writer respects. `unregisterProject` survives for the opposite case —
// forgetting a path entirely, mark included.
// ---------------------------------------------------------------------------

export interface ProjectEntry {
  /** Absolute filesystem path. Doubles as the entry's identity. */
  path: string;
  /** Display label — the trailing segment of `path`. */
  name: string;
  /** ISO-8601 timestamp the entry was registered (UTC). */
  addedAt: string;
  /** `true` when the operator took this folder off the sidebar list. The row
   *  still travels: an absence could not be told apart from a folder that was
   *  never registered, and the sidebar has to be able to offer it back. */
  hidden: boolean;
  /** Parent segment that tells this row apart from another one ending in the
   *  same name (`suzano` vs `suzano.old`). `null` when the name is unique.
   *  The SERVER decides this, not the UI: ambiguity is a property of the whole
   *  list, so it cannot be answered one row at a time. */
  parent: string | null;
}

interface RawProjectEntry {
  path: string;
  name: string;
  added_at: string;
  hidden: boolean;
  parent: string | null;
}

function toProjectEntry(raw: RawProjectEntry): ProjectEntry {
  return {
    path: raw.path,
    name: raw.name,
    addedAt: raw.added_at,
    hidden: raw.hidden,
    parent: raw.parent,
  };
}

export async function listRegisteredProjects(): Promise<ProjectEntry[]> {
  const raw = await call<RawProjectEntry[]>("dashboard_projects_list");
  return raw.map(toProjectEntry);
}

/** Register `path` and return the registry as it now stands. Registering an
 *  already-registered path is a no-op on the server, not an error.
 *
 *  A path the operator has hidden STAYS hidden through this call — the server
 *  funnels every automatic writer (init, the session observer) through it, so
 *  clearing the mark here would undo every removal. A deliberate "show this
 *  again" is [`unhideProject`]; the sidebar's manual add pairs the two. */
export async function registerProject(path: string): Promise<ProjectEntry[]> {
  const raw = await call<RawProjectEntry[]>("dashboard_projects_add", { path });
  return raw.map(toProjectEntry);
}

/** Take `path` off the sidebar list and return the registry as it now stands.
 *
 *  The row is MARKED, not dropped: every automatic writer would put a dropped
 *  row back, which is why removal used to only work for folders the operator
 *  did not have. This is the sidebar's removal gesture. */
export async function hideProject(path: string): Promise<ProjectEntry[]> {
  const raw = await call<RawProjectEntry[]>("dashboard_projects_hide", { path });
  return raw.map(toProjectEntry);
}

/** Clear the mark on `path` so it shows up on the list again. Unhiding a path
 *  the registry does not hold is a no-op on the server, not an error. */
export async function unhideProject(path: string): Promise<ProjectEntry[]> {
  const raw = await call<RawProjectEntry[]>("dashboard_projects_unhide", { path });
  return raw.map(toProjectEntry);
}

/** Forget `path` entirely — row and hidden mark alike — and return what
 *  remains. This is NOT how the sidebar removes a folder (see [`hideProject`]):
 *  without the mark, anything the discovery scan still finds comes back. */
export async function unregisterProject(path: string): Promise<ProjectEntry[]> {
  const raw = await call<RawProjectEntry[]>("dashboard_projects_remove", { path });
  return raw.map(toProjectEntry);
}
