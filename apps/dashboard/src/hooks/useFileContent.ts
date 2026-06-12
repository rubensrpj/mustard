import { useQuery } from "@tanstack/react-query";
import { fetchReadFile, type FileContent } from "@/lib/dashboard";

/**
 * Read one repository file's text + metadata for the code viewer. Backed by
 * the `dashboard_read_file` Tauri command, which is fail-open — a missing
 * file, a binary file, or a path that escapes the repo resolves to
 * `{ readable: false }` (never a rejected Promise), so callers render an empty
 * / "não foi possível abrir" state rather than catching.
 *
 * Disabled until both `repoPath` and `relPath` are set so the hook is safe to
 * mount before a file is chosen. Both sit at the queryKey leaves so the cache
 * keys per (project, file). This binding is the foundation for the next step
 * (wiring Git / most-touched / README / tracer to open files) — not yet wired.
 */
export function useFileContent(repoPath: string | null, relPath: string | null) {
  return useQuery<FileContent>({
    queryKey: ["read-file", repoPath, relPath],
    queryFn: () => fetchReadFile(repoPath as string, relPath as string),
    enabled: !!repoPath && !!relPath,
    staleTime: 30_000,
  });
}
