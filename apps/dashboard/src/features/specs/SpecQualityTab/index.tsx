import { relativeTime } from "@/lib/time";
import { extractTestLink } from "@/lib/quality-link";
import { StatusPill } from "@/components/specs/spec-status";
import type { SpecQualityItem } from "@/lib/types/specs";

interface SpecQualityTabProps {
  items: SpecQualityItem[];
  /** Absolute path to the project repo root. Required to build
   *  `vscode://file/...` URLs for the "abrir arquivo de teste" link.
   *  When `null` the button is omitted (we cannot resolve the absolute
   *  path on Windows without it). */
  repoPath: string | null;
}

function groupByWave(items: SpecQualityItem[]): [string, SpecQualityItem[]][] {
  const map = new Map<string, SpecQualityItem[]>();
  for (const item of items) {
    const key = item.wave != null ? `Onda ${item.wave}` : "Geral";
    const group = map.get(key) ?? [];
    group.push(item);
    map.set(key, group);
  }
  return Array.from(map.entries());
}

/**
 * Build a `vscode://file/<absPath>` URL. VS Code's URL handler accepts both
 * POSIX and Windows-style absolute paths; on Windows the path should be
 * `C:/Atiz/...` style (forward slashes are tolerated). We normalize to
 * forward slashes so the join works on both platforms.
 */
function buildEditorUrl(repoPath: string, relPath: string): string {
  const repo = repoPath.replace(/\\/g, "/").replace(/\/+$/, "");
  const rel = relPath.replace(/\\/g, "/").replace(/^\/+/, "");
  return `vscode://file/${repo}/${rel}`;
}

function AcRow({
  item,
  repoPath,
}: {
  item: SpecQualityItem;
  repoPath: string | null;
}) {
  const testLink = extractTestLink(item.command);
  const editorUrl = testLink && repoPath ? buildEditorUrl(repoPath, testLink) : null;

  return (
    <li className="rounded border border-border bg-card/30 p-3 flex flex-col gap-1.5">
      <div className="flex items-center gap-2 min-w-0">
        <StatusPill status={item.status} />
        <code className="text-[12px] font-mono font-medium shrink-0">
          {item.ac_id}
        </code>
        {item.ac_label && (
          <span className="text-[12px] text-foreground/80 truncate min-w-0">
            {item.ac_label}
          </span>
        )}
      </div>

      {item.command && (
        <div className="font-mono text-[11px] text-muted-foreground whitespace-pre-wrap break-all">
          <span className="text-muted-foreground/60">cmd:</span>{" "}
          <code>{item.command}</code>
        </div>
      )}

      {item.last_run_at && (
        <time
          className="text-[11px] text-muted-foreground"
          dateTime={item.last_run_at}
        >
          última execução: {relativeTime(item.last_run_at)}
        </time>
      )}

      {editorUrl && (
        // We deliberately use a plain anchor instead of a Tauri shell-open
        // call: the `vscode://` URL handler is registered by VS Code on
        // install, and Tauri's webview hands `target="_blank"` anchors with
        // unknown schemes to the OS shell, which routes them to VS Code.
        // This keeps the dependency surface flat (no new npm deps).
        <a
          href={editorUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="text-[11px] text-[--color-accent-mustard] hover:underline self-start"
          title={testLink ?? undefined}
        >
          abrir arquivo de teste
        </a>
      )}

      {item.status === "fail" && item.fail_reason && (
        <pre className="text-[--color-error] text-[11px] whitespace-pre-wrap mt-1">
          {item.fail_reason}
        </pre>
      )}
    </li>
  );
}

export function SpecQualityTab({ items, repoPath }: SpecQualityTabProps) {
  if (items.length === 0) {
    return (
      <p className="text-[13px] text-muted-foreground py-4 text-center">
        Nenhum critério de aceite registrado ainda.
      </p>
    );
  }

  const groups = groupByWave(items);

  return (
    <div className="flex flex-col gap-5">
      {groups.map(([wave, waveItems]) => (
        <section key={wave} className="flex flex-col gap-2">
          <h4 className="text-[10px] uppercase tracking-wide text-muted-foreground font-medium mb-1">
            {wave}
            <span
              className="text-muted-foreground/50 ml-1 tabular-nums"
              style={{ fontVariantNumeric: "tabular-nums" }}
            >
              {waveItems.filter((i) => i.status === "pass").length}/
              {waveItems.length}
            </span>
          </h4>
          <ul className="flex flex-col gap-2">
            {waveItems.map((item) => (
              <AcRow key={item.ac_id} item={item} repoPath={repoPath} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
