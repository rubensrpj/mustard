# Worktree isolation — one work unit, one worktree

Every work unit (feature, bugfix, any file change) runs in its OWN git worktree, so concurrent sessions never share a working tree. This REPLACES the old auto-branch (`git checkout -b {base}_{slug}` on the shared tree), which yanked the HEAD of every session pointed at the same directory.

## Per environment

- **Desktop** — the app already creates a worktree for EVERY session automatically; nothing to do. Its branch carries no `{base}_` prefix, so `/git` recovers the PR target through its fallback = the primary base (`git.flow["*"]`). For a non-primary base, pass the target to `/git` explicitly.
- **CLI, background** (`claude --bg`) — `settings.json#worktree.bgIsolation: "auto"` isolates each background session before its first edit. Automatic.
- **CLI, foreground** (live session) — NOT auto-isolated. Before the first edit of a work unit, isolate with the branch cut FROM THE RIGHT BASE — `EnterWorktree name={base}_{slug}` cuts from the repo's DEFAULT branch (`worktree.baseRef: fresh`), which is correct ONLY when `{base}` is the default. For any other base (e.g. work integrating into `dev` when the default is `main`):
  1. `git worktree add .claude/worktrees/{base}_{slug} -b {base}_{slug} origin/{base}` (fetch first),
  2. `EnterWorktree` with `path` pointing at it.
  The `{base}_` prefix is load-bearing: `/git` reads it to target the PR (`refs/git/git-flow.md`). `ExitWorktree` when done. Never `git checkout -b` on the shared tree.

## The gate

`work_branch_gate` (PreToolUse Write/Edit) detects isolation by comparing `git rev-parse --git-dir` with `--git-common-dir` (they differ ⇔ a linked worktree):

- **isolated** → allow; the worktree already carries its own branch (no checkout, which is what used to collide).
- **not isolated** → `MUSTARD_WORKTREE_ISOLATION_MODE` (`strict|warn|off`, default `warn`): `strict` denies with an `EnterWorktree` instruction; `warn` allows and advises; `off` is silent. Editing a bare integration base (`dev`/`main`) without a marker is denied regardless.
