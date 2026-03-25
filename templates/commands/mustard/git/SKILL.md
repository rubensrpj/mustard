# /git - Git Operations

> Commit, push, sync, PR, and deploy. Reads `mustard.json` for branch flow. Handles monorepo (submodules) and single repo automatically.

## Trigger

`/git <action>`

## Actions

| Action | Description |
|--------|-------------|
| `sync` | Pull parent branch into current branch |
| `commit` | Create commit (no push) |
| `push` | Sync + commit + push to remote |
| `merge` | Promote current → parent (PR if enabled, direct merge if not) |
| `deploy` | Push + merge + inform about cascade |

## Configuration

Reads `mustard.json` from the **project root**. If not found, falls back to defaults.

```json
{
  "git": {
    "flow": {
      "dev_*": "dev",
      "dev": "main"
    },
    "pr": {
      "enabled": true,
      "provider": "github"
    },
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

Promote current branch into its parent. Behavior depends on `pr.enabled`.

### Step 1 — Ensure pushed

Check if local is ahead of remote. If yes, execute `push` first.

### Step 2a — PR mode (`pr.enabled: true`)

Create a Pull Request from current → parent branch.

#### GitHub (`gh`)

```bash
gh pr create --base $PARENT --title "<title>" --body "<body>"
```

#### GitLab (`glab`)

```bash
glab mr create --target-branch $PARENT --title "<title>" --description "<body>"
```

#### PR Title & Body

- Title: conventional commit style — `<type>: <short description>`
- Body: auto-generated from commit log since divergence from parent:

```bash
git log $PARENT..HEAD --oneline
```

#### Monorepo Note

PRs are created at the **parent repo level only**. Submodules are committed and pushed, but PRs are not created per submodule (submodules follow parent via ref update).

### Step 2b — Direct mode (`pr.enabled: false`)

Merge locally and push.

#### Monorepo — submodules FIRST (PARALLEL, one Bash call each)

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git checkout $PARENT && git pull && git merge <branch> && git push && git checkout <branch>
```

#### Parent / Root

```bash
git checkout $PARENT && git pull && git merge <branch> && git push && git checkout <branch>
```

---

## deploy

Sequential phases. Each phase must complete before the next.

### Phase 1 — Push

Execute `push` action (includes sync).

### Phase 2 — Merge to parent

Execute `merge` action (current → parent).

### Phase 3 — Cascade (if parent ≠ production)

If `$PARENT` also has a parent in the flow (e.g., `dev` → `main`), inform the user:

> Merged/PR created: `dev_rubens → dev`. To promote `dev → main`, switch to `dev` and run `/git deploy` again.

Do NOT auto-cascade — each promotion is a deliberate decision.

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
