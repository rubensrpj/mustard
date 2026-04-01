# /git - Git Operations

> Commit, push, sync, and merge. Reads `mustard.json` for branch flow. Handles monorepo (submodules) and single repo automatically.

## Trigger

`/git <action>`

## Actions

| Action | Description |
|--------|-------------|
| `sync` | Pull parent branch into current branch |
| `commit` | Create commit (no push) |
| `push` | Commit + push to remote |
| `merge` | Sync + fast-forward merge to parent (single hop, always to dev) |
| `merge main` | Fast-forward merge dev → main (explicit promotion, must be on dev) |

## Configuration

Reads `mustard.json` from the **project root**. If not found, falls back to defaults.

```json
{
  "git": {
    "flow": {
      "*": "dev",
      "dev": "main"
    },
    "submodules": true
  }
}
```

### Flow Resolution

Match current branch against `flow` keys. Exact match first, then glob. `*` is the default fallback for any branch not explicitly listed.

| Current branch | Pattern matched | Parent resolved |
|---------------|----------------|-----------------|
| `feature/login` | `*` | `dev` |
| `fix/bug-123` | `*` | `dev` |
| `dev` | `dev` | `main` (only via `/git merge main`) |
| `main` | no match | **error**: terminal branch, no operations allowed |

**Rule**: Exact keys (`dev`, `main`) are matched first. `*` catches everything else. `main` and `dev` are never matched by `*`.

## Behavior

- **ZERO confirmations** — analyze, execute, done. NEVER ask for approval.
- **ZERO questions** — do NOT ask what to commit or whether to proceed.
- **Minimize Bash calls** — chain EVERYTHING with `&&`. One Bash call per repo max.
- **No investigation** — if a submodule is dirty, commit it.
- Submodules BEFORE parent (always).
- **Single repo**: skip all submodule steps — just operate on the root.
- **Local fast-forward merge** — no PRs, no merge commits, 100% linear history.

---

## Step 0 — Resolve Parent (all actions except commit)

```bash
cat mustard.json 2>/dev/null
git rev-parse --abbrev-ref HEAD
```

Match the current branch against `git.flow` patterns. Store as `$PARENT`.
If no match and no `mustard.json`: `$PARENT` = default branch (detect via `git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null || echo main`).

---

## Step 0b — Branch Protection Check

Before any operation (commit, push, merge, sync) check the current branch:

- If current branch is `main` → **REFUSE** with error: `Cannot operate directly on protected branch 'main'. Create a feature branch first.`
- If current branch is `dev` AND action is `commit`, `push`, or `sync` → **REFUSE** with error: `Cannot operate directly on protected branch 'dev'. Create a feature branch first.`
- If current branch is `dev` AND action is `merge main` → **ALLOW** (this is the only permitted operation on dev).

**Exception**: `/git merge main` is the sole operation allowed on the dev branch — it is the explicit promotion path.

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

**Branch check**: If on `main` or `dev` → refuse with error (see Step 0b).

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

**Branch check**: If on `main` or `dev` → refuse with error (see Step 0b).

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

Promote current branch into its parent via **local fast-forward merge** — no PRs, no merge commits, 100% linear history. Single hop only — always merges into `dev` (via `*` wildcard). Never cascades.

**Branch check**: If on `main` → refuse (terminal branch). If on `dev` → refuse (use `/git merge main` instead).

### Step 1 — Sync (mandatory)

Execute `sync` action to rebase from `dev`. If conflicts → STOP. Do not proceed to merge.

### Step 2 — Ensure pushed

Check if local is ahead of remote. If yes, execute `push` first.

### Step 3 — Merge into parent

`$SOURCE` = current branch, `$TARGET` = `$PARENT` (resolved in Step 0, always `dev` for feature/fix branches).

#### 3a. Submodules FIRST (PARALLEL — monorepo only)

For each submodule, pull both branches then fast-forward merge + push in ONE chained Bash call:

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git fetch origin && git checkout $SOURCE && git pull origin $SOURCE && git checkout $TARGET && git pull origin $TARGET && git merge $SOURCE --ff-only && git push origin $TARGET && git checkout $SOURCE
```

Skip submodules with no commits ahead (nothing to merge).

#### 3b. Parent repo

Same as submodules — pull both branches, then fast-forward merge + push:

```bash
git fetch origin && git checkout $SOURCE && git pull origin $SOURCE && git checkout $TARGET && git pull origin $TARGET && git merge $SOURCE --ff-only && git push origin $TARGET && git checkout $SOURCE
```

### Fast-forward failure

If `--ff-only` fails (branches diverged), STOP and report to user. This means someone pushed directly to the target branch — resolve manually.

### Example: `/git merge` from `feature/login`

```
feature/login → dev
  ├── SubprojectA:  ff-merged + pushed
  ├── SubprojectB:  ff-merged + pushed
  └── Parent:       ff-merged + pushed
```

---

## merge main

Explicit promotion of `dev` → `main`. **Must be on `dev` branch to use this.** This is the ONLY operation allowed on the dev branch.

**Branch check**: If NOT on `dev` → refuse with error: `'/git merge main' must be run from the dev branch. Currently on '<branch>'.`

### Step 1 — Ensure pushed

Check if local `dev` is ahead of remote. If yes, push first.

### Step 2 — Merge dev into main

`$SOURCE` = `dev`, `$TARGET` = `main`.

#### Submodules FIRST (PARALLEL — monorepo only)

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git fetch origin && git checkout dev && git pull origin dev && git checkout main && git pull origin main && git merge dev --ff-only && git push origin main && git checkout dev
```

Skip submodules with no commits ahead.

#### Parent repo

```bash
git fetch origin && git checkout dev && git pull origin dev && git checkout main && git pull origin main && git merge dev --ff-only && git push origin main && git checkout dev
```

### Fast-forward failure

If `--ff-only` fails, STOP and report. Resolve manually.

### Example: `/git merge main`

```
dev → main
  ├── SubprojectA:  ff-merged + pushed
  ├── SubprojectB:  ff-merged + pushed
  └── Parent:       ff-merged + pushed
```

### Output

Print a summary table at the end:

```
| Repo            | Status             |
|-----------------|--------------------|
| SubprojectA     | ff-merged + pushed |
| SubprojectB     | ff-merged + pushed |
| SubprojectC     | skipped            |
| Parent          | ff-merged + pushed |
```

---

## Cautions

- Aborts if ANY repo has merge conflicts (sync or push)
- Aborts if `--ff-only` fails (branches diverged)
- Submodules BEFORE parent (in sync, push, commit, and merge)
- NEVER use `git add .` — use `git add -A` from the correct directory
- If any operation fails, stop and report
- After merge, return to the original branch (`$SOURCE`)
- NEVER commit, push, or sync directly on `main` or `dev`
- `/git merge main` is the ONLY operation permitted on dev

## Performance Budget

- **Max Task agents**: 1 per dirty submodule
- **Max Bash calls per agent**: 1 (all commands chained)
- **Max Bash calls for merge**: 1 per submodule + 1 for parent
