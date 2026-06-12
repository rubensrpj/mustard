import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { GitBranch, Check, ChevronDown } from "lucide-react";
import { DataCard, SectionHeader, StatPill, EmptyState } from "@/components/page";
import {
  DropdownMenu,
  DropdownMenuTrigger,
  DropdownMenuContent,
  DropdownMenuItem,
} from "@/components/ui/dropdown-menu";
import { useGitInfo } from "@/hooks/useGitInfo";
import {
  fetchGitLog,
  type GitInfo,
  type GitChangedFile,
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
/** How many change rows render before the rest collapse to "+N mais". */
const MAX_CHANGE_ROWS = 12;

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

/** Split a repo-relative path into its directory and basename for a GitLens-
 *  style row (basename emphasised, directory muted/mono). */
function splitPath(path: string): { dir: string; name: string } {
  const norm = path.replace(/\\/g, "/").replace(/\/$/, "");
  const i = norm.lastIndexOf("/");
  if (i < 0) return { dir: "", name: norm };
  return { dir: norm.slice(0, i), name: norm.slice(i + 1) };
}

/** The single status glyph + tonal color for a changed file. Untracked reads
 *  muted "U"; a purely-staged file green "A"; anything with an unstaged edit
 *  amber "M". A file that is both staged and unstaged keeps the amber "M" and
 *  earns a small green dot (it carries staged work too). */
function statusBadge(c: GitChangedFile): {
  letter: string;
  color: TonalColor;
  staged: boolean;
} {
  if (c.untracked) return { letter: "U", color: TONE.muted, staged: false };
  if (c.unstaged) return { letter: "M", color: TONE.warning, staged: c.staged };
  if (c.staged) return { letter: "A", color: TONE.success, staged: true };
  return { letter: "M", color: TONE.warning, staged: false };
}

/** One changed-file row — basename in the foreground, directory mono/muted, and
 *  the colored status letter (with a staged dot) right-aligned. */
function ChangeRow({ change }: { change: GitChangedFile }) {
  const { dir, name } = splitPath(change.path);
  const { letter, color, staged } = statusBadge(change);
  return (
    <li
      className="flex items-center gap-2 text-[12.5px]"
      title={change.path}
    >
      <div className="flex min-w-0 flex-1 items-baseline gap-1.5">
        <span className="shrink-0 truncate font-medium text-foreground/90">
          {name}
        </span>
        {dir && (
          <span className="truncate font-mono text-[11px] text-muted-foreground/70">
            {dir}
          </span>
        )}
      </div>
      <span className="flex shrink-0 items-center gap-1">
        {staged && (
          <span
            aria-hidden
            className="h-1.5 w-1.5 rounded-full"
            style={{ backgroundColor: TONE.success }}
          />
        )}
        <span
          className="font-mono text-[11px] font-semibold tabular-nums"
          style={{ color }}
        >
          {letter}
        </span>
      </span>
    </li>
  );
}

/** A single commit row in the git-graph history: a solid colored rail + node on
 *  the left connecting the commits, the conventional-type badge, hash, subject
 *  and author·date. The HEAD (topmost) commit gets a highlight ring. */
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
      {/* Solid colored rail + node — the continuous "git graph" trail. */}
      <span className="relative flex w-3 shrink-0 justify-center" aria-hidden>
        <span
          className="absolute left-1/2 w-px -translate-x-1/2 bg-border"
          style={{
            top: first ? "0.55rem" : 0,
            bottom: last ? "auto" : 0,
            height: last ? "0.55rem" : undefined,
          }}
        />
        <span
          className={cn(
            "relative mt-1 h-2.5 w-2.5 shrink-0 rounded-full ring-2 ring-[--card]",
            first && "ring-[3px]",
          )}
          style={{
            backgroundColor: color,
            ...(first
              ? {
                  boxShadow: `0 0 0 2px color-mix(in srgb, ${color} 55%, transparent)`,
                }
              : null),
          }}
        />
      </span>
      <div className="flex min-w-0 flex-col gap-0.5 pb-2.5">
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

/** Compact branch picker — a "branch ▾" trigger opening the local branch list.
 *  Selecting a branch drives the history graph below; the current HEAD branch
 *  is marked. Falls back to a static label when there is only one branch. */
function BranchPicker({
  branches,
  current,
  selected,
  onSelect,
}: {
  branches: string[];
  current: string;
  selected: string;
  onSelect: (b: string) => void;
}) {
  const t = useT();
  if (branches.length <= 1) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 font-mono text-[12px] text-foreground/85">
        <GitBranch className="h-3.5 w-3.5 text-[--accent]" aria-hidden />
        {selected || current || t("overview.git.detached", "HEAD destacado")}
      </span>
    );
  }
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className={cn(
            "inline-flex items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 font-mono text-[12px] text-foreground/85 transition-colors hover:bg-muted/40",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[--primary]",
          )}
        >
          <GitBranch className="h-3.5 w-3.5 text-[--accent]" aria-hidden />
          <span className="max-w-[160px] truncate">{selected}</span>
          <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" aria-hidden />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="max-h-[280px] min-w-[200px]">
        {branches.map((b) => {
          const isCurrent = b === current;
          const isSelected = b === selected;
          return (
            <DropdownMenuItem
              key={b}
              onSelect={() => onSelect(b)}
              className="gap-2 font-mono text-[12px]"
            >
              <GitBranch
                className={cn(
                  "h-3.5 w-3.5 shrink-0",
                  isCurrent ? "text-[--accent]" : "text-transparent",
                )}
                aria-hidden
              />
              <span className="min-w-0 flex-1 truncate">{b}</span>
              {isSelected && (
                <Check className="h-3.5 w-3.5 shrink-0 text-[--accent]" aria-hidden />
              )}
            </DropdownMenuItem>
          );
        })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function GitBody({ data, repoPath }: { data: GitInfo; repoPath: string }) {
  const t = useT();
  const branches = data.branches;
  const changes = data.changes ?? [];
  const totalPending =
    data.pending.staged + data.pending.unstaged + data.pending.untracked;
  const hiddenChanges = Math.max(0, totalPending - changes.length);

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
    <div className="mt-3 flex flex-col gap-5">
      {/* Repo context — branch picker + ahead/behind. */}
      <div className="flex flex-wrap items-center gap-2">
        <BranchPicker
          branches={branches}
          current={data.branch}
          selected={selectedBranch}
          onSelect={setSelectedBranch}
        />
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

      {/* Changes — GitLens-style working-tree file list. */}
      <div className="flex flex-col gap-2">
        <SectionHeader
          title={t("overview.git.changes", "Alterações")}
          right={
            totalPending > 0 ? (
              <span className="font-mono tabular-nums">{totalPending}</span>
            ) : undefined
          }
        />
        {changes.length === 0 ? (
          <span className="inline-flex items-center gap-2 text-[12.5px] text-[--intent-success]">
            <TonalIcon icon={Check} color={TONE.success} />
            {t("overview.git.clean", "working tree limpo")}
          </span>
        ) : (
          <>
            <ul className="flex flex-col gap-1.5">
              {changes.slice(0, MAX_CHANGE_ROWS).map((c) => (
                <ChangeRow key={c.path} change={c} />
              ))}
            </ul>
            {(changes.length > MAX_CHANGE_ROWS || hiddenChanges > 0) && (
              <span className="text-[11px] text-muted-foreground/70">
                {t("overview.git.moreChanges", "+{n} mais").replace(
                  "{n}",
                  String(
                    changes.length - Math.min(changes.length, MAX_CHANGE_ROWS) +
                      hiddenChanges,
                  ),
                )}
              </span>
            )}
          </>
        )}
      </div>

      {/* Graph — the selected branch's commit history, the card's protagonist. */}
      {commits.length > 0 && (
        <div className="flex flex-col gap-2">
          <SectionHeader
            title={t("overview.git.history", "Histórico")}
            right={
              selectedBranch ? (
                <span className="font-mono normal-case">{selectedBranch}</span>
              ) : undefined
            }
          />
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
 * Local git state card for the workspace overview, in the GitLens / VS Code
 * Source Control spirit: a compact repo-context header (branch picker +
 * ahead/behind, remote in the section header), a GitLens-style "Alterações"
 * list of working-tree files (basename + dir + colored status letter, total in
 * the header), and the commit-graph history of the selected branch as the
 * protagonist (a continuous colored rail keyed on the conventional-commit type,
 * HEAD highlighted). Backed by `useGitInfo` (local `git`, no network / `gh`) +
 * the `dashboard_git_log` per-branch log. Fail-open — a non-repo path resolves
 * to `is_repo: false`, rendering an empty state instead of an error.
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
