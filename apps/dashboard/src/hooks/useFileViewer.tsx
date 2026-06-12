import { useCallback, useState } from "react";
import { CodeViewer } from "@/components/page/CodeViewer";
import { useFileContent } from "@/hooks/useFileContent";

/** The file currently requested for viewing. `relPath` is whatever the caller
 *  passed (repo-relative OR absolute — `dashboard_read_file` resolves both with
 *  containment); `fileName` is the header label (basename when not supplied). */
interface OpenFile {
  relPath: string;
  fileName: string;
}

/** Derive a display basename from a path with either separator. */
function basename(path: string): string {
  const norm = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const i = norm.lastIndexOf("/");
  return i < 0 ? norm : norm.slice(i + 1);
}

/**
 * Reusable launcher for the `CodeViewer` modal — DRY across the Git card,
 * most-touched ranking, and the tracer. Owns the open-file state, fetches its
 * content via `useFileContent(repoPath, relPath)` (TanStack Query, disabled
 * until both are set), and returns a ready-wired `<CodeViewer>` element.
 *
 * Usage: a card calls `openFile(path)` from a click handler and renders
 * `{viewer}` once. Closing the modal zeroes the state (which disables the
 * fetch). `fileName` falls back to the basename of the path when omitted.
 */
export function useFileViewer(repoPath: string | null) {
  const [openFileState, setOpenFileState] = useState<OpenFile | null>(null);

  const openFile = useCallback((relPath: string, fileName?: string) => {
    if (!relPath) return;
    setOpenFileState({ relPath, fileName: fileName ?? basename(relPath) });
  }, []);

  const { data } = useFileContent(repoPath, openFileState?.relPath ?? null);

  const viewer = (
    <CodeViewer
      open={openFileState != null}
      onOpenChange={(open) => {
        if (!open) setOpenFileState(null);
      }}
      fileName={openFileState?.fileName ?? ""}
      content={data?.content ?? ""}
      language={data?.language ?? ""}
      isBinary={data?.is_binary}
      truncated={data?.truncated}
      readable={data?.readable}
    />
  );

  return { openFile, viewer };
}
