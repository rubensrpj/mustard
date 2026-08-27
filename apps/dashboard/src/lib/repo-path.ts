/**
 * Path-containment helper shared by the trace ("abrir" affordance) and the
 * docked CodeViewer. The dashboard's `dashboard_read_file` command is a
 * deliberate sandbox: it only reads files INSIDE the project repo and rejects
 * anything outside (job temp dirs, `~/.claude/jobs/...`, `/tmp`, …) with
 * `readable: false`. Tool-trace events, however, carry absolute paths that
 * frequently point at those ephemeral out-of-repo artifacts (a `plan.json`
 * written to the job tmp, a dispatch stub, a commit-message file). Offering an
 * "abrir" button for a file that can never be read — and then showing a generic
 * "could not open" — is the confusing UX this guards against.
 *
 * `isPathInsideRepo` answers "would `dashboard_read_file` accept this path?"
 * purely client-side, so the caller can hide the affordance (or show a precise
 * reason) BEFORE round-tripping to a guaranteed failure.
 */

/** Normalize separators to `/` and drop any trailing slash. */
function norm(p: string): string {
  return p.replace(/\\/g, "/").replace(/\/+$/, "");
}

/** Does `p` look absolute? A Windows drive (`C:/…`), a UNC/POSIX root (`/…`),
 *  or a Windows verbatim prefix (`//?/C:/…` after normalization). */
function isAbsolute(p: string): boolean {
  return /^[a-zA-Z]:\//.test(p) || p.startsWith("/");
}

/** Case-fold a path for comparison. Windows paths are case-insensitive, so we
 *  lowercase a drive-rooted path; POSIX paths stay case-sensitive. */
function fold(p: string): string {
  return /^[a-zA-Z]:\//.test(p) ? p.toLowerCase() : p;
}

/**
 * `true` when `filePath` resolves inside `repoPath` (i.e. the read sandbox would
 * accept it). A RELATIVE `filePath` is treated as repo-relative → inside. An
 * ABSOLUTE `filePath` must equal the repo root or sit under it. Returns `false`
 * when either input is empty. This is a heuristic mirror of the Rust
 * `resolve_within_repo` containment check — it does NOT canonicalize symlinks or
 * resolve `..`, so it is intentionally conservative (a `..` that escapes still
 * reads as "inside" here, but the backend rejects it — fail-safe either way).
 */
export function isPathInsideRepo(filePath: string, repoPath: string): boolean {
  if (!filePath || !repoPath) return false;
  const f = norm(filePath);
  const r = norm(repoPath);
  // A repo-relative path (no drive, no leading slash) is inside by definition.
  if (!isAbsolute(f)) return true;
  const ff = fold(f);
  const rf = fold(r);
  return ff === rf || ff.startsWith(rf + "/");
}
