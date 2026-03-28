# /git - Git Operations

> Commit, push, sync, PR, and deploy. Reads `mustard.json` for branch flow. Handles monorepo (submodules) and single repo automatically.

## Trigger

`/git <action>`

## Actions

| Action | Description |
|--------|-------------|
| `sync` | Pull parent branch into current branch |
| `commit` | Create commit (no push) |
| `push` | Commit + push to remote |
| `merge` | Push + cascade PRs to parent (dev_rubens → dev) |
| `merge main` | Cascade PRs from dev → main (explicit, when ready) |

## Configuration

Reads `mustard.json` from the **project root**. If not found, falls back to defaults.

```json
{
  "git": {
    "flow": {
      "dev_*": "dev",
      "dev": "main"
    },
    "provider": "github",
    "submodules": true
  }
}
```

### Flow Resolution

Match current branch against `flow` keys (glob patterns). First match wins.

| Current branch | Pattern matched | Parent resolved |
|---------------|----------------|-----------------|
| `dev_rubens` | `dev_*` | `dev` |
| `dev` | `dev` | `main` |
| `feature/xyz` | no match | **error**: no parent configured |

**Fallback** (no `mustard.json`): parent = default branch (`main` or `master`).

### Provider CLI

| Provider | CLI | PR command |
|----------|-----|------------|
| `github` | `gh` | `gh pr create` |
| `gitlab` | `glab` | `glab mr create` |
| `bitbucket` | `bb` | `bb pr create` (manual) |

## Behavior

- **ZERO confirmations** — analyze, execute, done. NEVER ask for approval.
- **ZERO questions** — do NOT ask what to commit or whether to proceed.
- **Minimize Bash calls** — chain EVERYTHING with `&&`. One Bash call per repo max.
- **No investigation** — if a submodule is dirty, commit it.
- Submodules BEFORE parent (always).
- **Single repo**: skip all submodule steps — just operate on the root.

---

## Step 0 — Resolve Parent (all actions except commit)

```bash
cat mustard.json 2>/dev/null
git rev-parse --abbrev-ref HEAD
```

Match the current branch against `git.flow` patterns. Store as `$PARENT`.
If no match and no `mustard.json`: `$PARENT` = default branch (detect via `git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null || echo main`).

Read `git.provider` from `mustard.json`. Fallback: read `git.pr.provider` (old format). Default: `github`.

---

## sync

Pull the parent branch changes into the current branch.

### Single repo / Parent repo

```bash
git fetch origin $PARENT && git rebase origin/$PARENT
```

### Monorepo — submodules FIRST (PARALLEL, one Bash call each)

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git fetch origin $PARENT && git rebase origin/$PARENT
```

Then parent repo (same command at root).

If rebase has conflicts → abort rebase, report to user, STOP.

---

## commit

### 1. Analyze all changes (single parallel batch)

Run in **one parallel batch**:
- `git status`
- `git submodule status` (skip if no `.gitmodules`)
- `git diff`
- `git log --oneline -5`

### 2. Commit dirty submodules (if any — monorepo only)

Launch **ONE parallel Task agent per dirty submodule** (`model: "haiku"`). Each agent runs ONE chained Bash command:

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git add -A && git diff --cached --stat && git commit -m "<message>"
```

### 3. Commit parent repo

```bash
git add -A && git diff --cached --stat && git commit -m "<message>"
```

### Message Format

```
<type>: <short description>

<detailed description if needed>

Co-Authored-By: Claude <noreply@anthropic.com>
```

Types: feat, fix, refactor, docs, chore, test

---

## push

Sequential: **sync first**, then commit + push.

### Phase 1 — Sync

Execute `sync` action. If conflicts → STOP.

### Phase 2 — Commit & Push

#### Submodules (PARALLEL — monorepo only, one Bash call each)

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git add -A && git commit -m "<message>" && git push origin <branch>
```

#### Parent / Root (ONE Bash call)

```bash
git add -A && git commit -m "<message>" && git push origin <branch>
```

---

## merge

Promote current branch into its parent via Pull Request — **recursively through the entire flow chain**, including all submodules.

### Step 1 — Ensure pushed

Check if local is ahead of remote. If yes, execute `push` first.

### Step 2 — Cascade merge (recursive)

Resolve the **full flow chain** from the current branch to the terminal branch (the one with no parent in the flow). Example: `dev_rubens` → `dev` → `main` = 2 hops.

**For each hop** (e.g., first `dev_rubens → dev`, then `dev → main`):

#### 2a. Submodules FIRST (PARALLEL — monorepo only)

For each submodule that has commits ahead of `$TARGET`:

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git fetch origin && git log origin/$TARGET..origin/$SOURCE --oneline
```

If commits exist, create PR and merge immediately:

```bash
# GitHub
cd <SUBMODULE_ABSOLUTE_PATH> && gh pr create --head $SOURCE --base $TARGET --title "<title>" --body "<body>"
cd <SUBMODULE_ABSOLUTE_PATH> && gh pr merge --merge
```

```bash
# GitLab
cd <SUBMODULE_ABSOLUTE_PATH> && glab mr create --source-branch $SOURCE --target-branch $TARGET --title "<title>" --description "<body>" --remove-source-branch=false
cd <SUBMODULE_ABSOLUTE_PATH> && glab mr merge
```

Skip submodules with no commits ahead (nothing to merge).

#### 2b. Parent repo

Same as submodules — create PR + merge for this hop:

```bash
# GitHub
gh pr create --head $SOURCE --base $TARGET --title "<title>" --body "<body>"
gh pr merge --merge
```

#### 2c. Next hop

After all PRs are merged for this hop, advance to the next hop in the chain. Repeat 2a–2b.

**Auto-merge stops at the first parent** (e.g., `dev_rubens → dev`). The final promotion to the terminal branch (e.g., `dev → main`) is **never automatic** — it requires a separate explicit `/git merge main` call.

### PR Title & Body

- Title: conventional commit style — `<type>: <short description>`
- Body: auto-generated from commit log since divergence:

```bash
git log $TARGET..$SOURCE --oneline
```

### Example: `/git merge` from `dev_rubens`

```
dev_rubens → dev (auto)
  ├── Competi.Backend:  PR #N created + merged
  ├── Competi.Frontend: PR #N created + merged
  ├── Competi.Libs:     PR #N created + merged
  ├── Competi.Admin:    (skipped — no commits ahead)
  └── Competi.CRM:      PR #N created + merged
```

### Example: `/git merge main` (explicit — when ready for production)

```
dev → main
  ├── Competi.Backend:  PR #N created + merged
  ├── Competi.Frontend: PR #N created + merged
  ├── Competi.Libs:     PR #N created + merged
  └── Competi.CRM:      PR #N created + merged
```

### Output

Print a summary table at the end:

```
| Repo            | Status          |
|-----------------|-----------------|
| Backend         | PR #1 merged    |
| Frontend        | PR #1 merged    |
| Libs            | PR #1 merged    |
| Admin           | skipped         |
| CRM (parent)    | PR #3 merged    |
```

---

## Cautions

- Aborts if ANY repo has merge conflicts (sync or push)
- Submodules BEFORE parent (in sync, push, and commit)
- NEVER use `git add .` — use `git add -A` from the correct directory
- If any operation fails, stop and report
- PR creation requires the provider CLI to be installed and authenticated

## Performance Budget

- **Max Task agents**: 1 per dirty submodule
- **Max Bash calls per agent**: 1 (all commands chained)
- **Max API calls total**: ≤ 12

ULTRATHINK
