import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  GitBranch,
  FileDiff,
  FileQuestion,
  Check,
  type LucideIcon,
} from "lucide-react";
import { DataCard, SectionHeader, StatPill, EmptyState } from "@/components/page";
import { useGitInfo } from "@/hooks/useGitInfo";
import {
  fetchGitLog,
  type GitInfo,
  type CommitSummary,
} from "@/lib/dashboard";
import { relativeTime } from "@/lib/time";
import {
  TonalIcon,
  TONE,
  tonalStyle,
  commitType,
  type TonalColor,
} from "@/features/workspace/_shared/tonal";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

interface GitInfoCardProps {
  repoPath: string;
}

/** How many commits the selected-branch history requests. */
const GIT_LOG_LIMIT = 30;

/** Strip the `.git` suffix and protocol/host noise from a remote URL so the
 *  card shows a compact `owner/repo` style identifier when possible. */
function shortRemote(url: string): string {
  if (!url) return "";
  const s = url.replace(/\.git$/, "");
  // git@host:owner/repo  →  owner/repo
  const scp = s.match(/^[^@]+@[^:]+:(.+)$/);
  if (scp) return scp[1];
  // https://host/owner/repo  →  owner/repo
  const http = s.match(/^https?:\/\/[^/]+\/(.+)$/);
  if (http) return http[1];
  return s;
}

/** One pending-status row (staged / unstaged / untracked) with a tonal icon. */
function PendingItem({
  icon,
  color,
  count,
  label,
}: {
  icon: LucideIcon;
  color: TonalColor;
  count: number;
  label: string;
}) {
  const active = count > 0;
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 text-[12px]",
        active ? "text-foreground/80" : "text-muted-foreground/60",
      )}
      title={label}
    >
      <TonalIcon icon={icon} color={active ? color : TONE.muted} />
      <span className="font-mono tabular-nums">{count}</span>
      <span>{label}</span>
    </span>
  );
}

/** A single commit row in the git-graph style history: a colored rail/dot on
 *  the left, the conventional-type badge, hash, subject, author·date. */
function CommitRow({
  commit,
  first,
  last,
}: {
  commit: CommitSummary;
  first: boolean;
  last: boolean;
}) {
  const { type, color } = commitType(commit.subject);
  return (
    <li className="flex items-stretch gap-2.5 text-[12.5px]">
      {/* Colored rail + node — the "git graph" trail connecting commits. */}
      <span className="relative flex w-3 shrink-0 justify-center" aria-hidden>
        <span
          className="absolute left-1/2 w-px -translate-x-1/2"
          style={{
            backgroundColor: `color-mix(in srgb, ${color} 35%, transparent)`,
            top: first ? "0.5rem" : 0,
            bottom: last ? "auto" : 0,
            height: last ? "0.5rem" : undefined,
          }}
        />
        <span
          className="relative mt-1 h-2 w-2 shrink-0 rounded-full ring-2 ring-[--card]"
          style={{ backgroundColor: color }}
        />
      </span>
      <div className="flex min-w-0 flex-col gap-0.5 pb-2">
        <div className="flex items-center gap-1.5">
          {type && (
            <span
              className="shrink-0 rounded px-1 py-px font-mono text-[10px] font-medium uppercase tracking-wide"
              style={tonalStyle(color)}
            >
              {type}
            </span>
          )}
          <span className="truncate text-foreground/85" title={commit.subject}>
            <span className="font-mono text-[--accent]">{commit.hash}</span>{" "}
            {commit.subject}
          </span>
        </div>
        <span className="text-[11px] text-muted-foreground/70">
          {commit.author}
          {commit.date ? ` · ${relativeTime(commit.date)}` : ""}
        </span>
      </div>
    </li>
  );
}

function GitBody({ data, repoPath }: { data: GitInfo; repoPath: string }) {
  const t = useT();
  const pending = data.pending;
  const cleanTree =
    pending.staged === 0 && pending.unstaged === 0 && pending.untracked === 0;
  const branches = data.branches;

  // Selected branch drives the history; defaults to the current HEAD branch.
  const [selectedBranch, setSelectedBranch] = useState<string>(data.branch);

  // Fetch the selected branch's log. Keyed on (repoPath, branch) so each branch
  // caches independently; disabled until both are known. While no branch is
  // selectable (detached HEAD), fall back to the HEAD recent_commits below.
  const { data: branchLog } = useQuery<CommitSummary[]>({
    queryKey: ["git-log", repoPath, selectedBranch],
    queryFn: () => fetchGitLog(repoPath, selectedBranch, GIT_LOG_LIMIT),
    enabled: !!repoPath && !!selectedBranch,
    staleTime: 30_000,
  });

  // Use the selected-branch log when available; otherwise the HEAD commits the
  // info call already returned (initial paint, or detached HEAD).
  const commits: CommitSummary[] =
    branchLog ?? data.recent_commits.map((c) => ({ ...c }));

  return (
    <div className="mt-3 flex flex-col gap-4">
      {/* Branch + ahead/behind */}
      <div className="flex flex-wrap items-center gap-2">
        {data.branch ? (
          <span className="inline-flex items-center gap-2 text-[13px] text-foreground/90">
            <TonalIcon icon={GitBranch} color={TONE.accent} />
            <span className="font-mono font-medium">{data.branch}</span>
          </span>
        ) : (
          <span className="inline-flex items-center gap-2 text-[13px] text-muted-foreground/70">
            <TonalIcon icon={GitBranch} color={TONE.muted} />
            {t("overview.git.detached", "HEAD destacado")}
          </span>
        )}
        {data.ahead > 0 && (
          <StatPill
            value={data.ahead}
            unit="↑"
            intent="success"
            tooltip={t("overview.git.aheadTooltip", "commits à frente do upstream")}
          />
        )}
        {data.behind > 0 && (
          <StatPill
            value={data.behind}
            unit="↓"
            intent="warning"
            tooltip={t("overview.git.behindTooltip", "commits atrás do upstream")}
          />
        )}
        {data.ahead === 0 && data.behind === 0 && data.branch && (
          <span className="text-[11px] text-muted-foreground/70">
            {t("overview.git.upToDate", "em dia")}
          </span>
        )}
      </div>

      {/* Pending — working-tree state */}
      <div className="flex flex-col gap-1.5">
        <span className="text-[11px] uppercase tracking-wider text-muted-foreground">
          {t("overview.git.pending", "Pendências")}
        </span>
        {cleanTree ? (
          <span className="inline-flex items-center gap-2 text-[12.5px] text-[--intent-success]">
            <TonalIcon icon={Check} color={TONE.success} />
            {t("overview.git.clean", "working tree limpo")}
          </span>
        ) : (
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
            <PendingItem
              icon={Check}
              color={TONE.success}
              count={pending.staged}
              label={t("overview.git.staged", "staged")}
            />
            <PendingItem
              icon={FileDiff}
              color={TONE.warning}
              count={pending.unstaged}
              label={t("overview.git.unstaged", "modificados")}
            />
            <PendingItem
              icon={FileQuestion}
              color={TONE.muted}
              count={pending.untracked}
              label={t("overview.git.untracked", "não rastreados")}
            />
          </div>
        )}
      </div>

      {/* Branches — clickable; the selected one drives the history below. */}
      {branches.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <span className="text-[11px] uppercase tracking-wider text-muted-foreground">
            {t("overview.git.branches", "Branches")}
          </span>
          <div className="flex flex-wrap gap-1.5">
            {branches.map((b) => {
              const selected = b === selectedBranch;
              const current = b === data.branch;
              return (
                <button
                  key={b}
                  type="button"
                  onClick={() => setSelectedBranch(b)}
                  title={
                    current
                      ? t("overview.git.currentBranch", "branch atual")
                      : b
                  }
                  className={cn(
                    "inline-flex items-center gap-1 rounded-full border px-2 py-0.5 font-mono text-[11px] transition-colors",
                    "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
                    selected
                      ? "text-[--accent]"
                      : "border-border bg-card text-muted-foreground hover:bg-muted/40",
                  )}
                  style={
                    selected
                      ? {
                          color: TONE.accent,
                          borderColor: `color-mix(in srgb, ${TONE.accent} 60%, transparent)`,
                          backgroundColor: `color-mix(in srgb, ${TONE.accent} 15%, transparent)`,
                        }
                      : undefined
                  }
                >
                  {current && <GitBranch className="h-3 w-3" aria-hidden />}
                  {b}
                </button>
              );
            })}
          </div>
        </div>
      )}

      {/* History — colored git-graph for the selected branch. */}
      {commits.length > 0 && (
        <div className="flex flex-col gap-1.5">
          <span className="text-[11px] uppercase tracking-wider text-muted-foreground">
            {t("overview.git.history", "Histórico")}
            {selectedBranch ? (
              <span className="ml-1.5 font-mono normal-case text-muted-foreground/70">
                {selectedBranch}
              </span>
            ) : null}
          </span>
          <ul className="flex flex-col">
            {commits.map((c, i) => (
              <CommitRow
                key={c.hash}
                commit={c}
                first={i === 0}
                last={i === commits.length - 1}
              />
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

/**
 * Local git state card for the workspace overview — a compact git client:
 * current branch + ahead/behind vs upstream, working-tree pending counts
 * (staged / unstaged / untracked, or "working tree limpo"), a clickable local
 * branch list and the recent-commit history of the selected branch (a colored
 * git-graph keyed on the conventional-commit type). Backed by `useGitInfo`
 * (local `git`, no network / `gh`) + the `dashboard_git_log` per-branch log.
 * Fail-open — a non-repo path resolves to `is_repo: false`, rendering an empty
 * state instead of an error.
 */
export function GitInfoCard({ repoPath }: GitInfoCardProps) {
  const t = useT();
  const { data } = useGitInfo(repoPath);

  if (!data || !data.is_repo) {
    return (
      <DataCard padded>
        <SectionHeader title={t("overview.git.title", "Git")} />
        <EmptyState
          className="mt-3"
          title={t("overview.git.empty.title", "Sem repositório git")}
          description={t(
            "overview.git.empty.description",
            "Este workspace não está dentro de um repositório git.",
          )}
        />
      </DataCard>
    );
  }

  const remote = shortRemote(data.remote_url);

  return (
    <DataCard padded>
      <SectionHeader
        title={t("overview.git.title", "Git")}
        right={
          remote ? (
            <span
              className="font-mono text-[11px] text-muted-foreground truncate max-w-[220px]"
              title={data.remote_url}
            >
              {remote}
            </span>
          ) : (
            <span className="text-[11px] text-muted-foreground/70">
              {t("overview.git.noRemote", "sem remote")}
            </span>
          )
        }
      />
      <GitBody data={data} repoPath={repoPath} />
    </DataCard>
  );
}
