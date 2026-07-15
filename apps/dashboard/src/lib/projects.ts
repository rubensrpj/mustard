// Project-registry invoke wrappers (B6 Wave 1).
//
// Single import site for the dashboard's per-project install/detection
// surface. Components MUST NOT call `invoke()` directly — they import from
// here.
//
// `mustard_update` is an existing Tauri command wired in `src-tauri/src/lib.rs`
// (B5 Wave 3); it calls `mustard_cli::update` natively, no sidecar process. We
// re-wrap it under the projects/ surface so the registry UI has one import.

import { invoke } from "@tauri-apps/api/core";

export interface ProjectDetection {
  /** True when `<path>/.claude/CLAUDE.md` exists. */
  installed: boolean;
  /** Mustard CLI version stamped into `<path>/.claude/mustard.json`, when
   *  readable. `null` when the file is missing or malformed. */
  version: string | null;
}

export function detectProjectMustard(path: string): Promise<ProjectDetection> {
  return invoke<ProjectDetection>("detect_project_mustard", { path });
}

export function updateMustard(path: string): Promise<void> {
  return invoke<void>("mustard_update", { path });
}

export function uninstallMustard(path: string): Promise<void> {
  return invoke<void>("uninstall_mustard", { path });
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

// Tauri returns snake_case field names by default (see `#[serde(rename_all =
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
  const raw = await invoke<RawArtifactDriftReport>("artifact_update_check", {
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
  const raw = await invoke<RawArtifactUpdateOutcome>("artifact_update_apply", {
    projectPath,
  });
  return { applied: raw.applied, manifestWritten: raw.manifest_written };
}

export function isMustardRepo(projectPath: string): Promise<boolean> {
  return invoke<boolean>("is_mustard_repo", { projectPath });
}
