# /git - Git Operations

> Commit, push, merge, and deploy. Handles monorepo (submodules) and single repo automatically.

## Trigger

`/git <action>`

## Actions

| Action | Description |
|--------|-------------|
| `commit` | Create commit (no push) |
| `push` | Commit + push to remote |
| `merge` | Merge current branch to main |
| `deploy` | Push + merge to main (full deploy) |

## Behavior

- **ZERO confirmations** — analyze, execute, done. NEVER ask for approval.
- **ZERO questions** — do NOT ask what to commit or whether to proceed.
- **Minimize Bash calls** — chain EVERYTHING with `&&`. One Bash call per repo max.
- **No investigation** — if a submodule is dirty, commit it.
- Submodules BEFORE parent (always).
- **Single repo**: skip all submodule steps — just commit/push/merge the root.

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

Same as `commit`, but each step also pushes:

### Submodules (PARALLEL — monorepo only, one Bash call each)

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git add -A && git commit -m "<message>" && git push origin <branch>
```

### Parent / Root (ONE Bash call)

```bash
git add -A && git commit -m "<message>" && git push origin <branch>
```

---

## merge

### Step 1 — Detect context

```bash
git rev-parse --abbrev-ref HEAD && git submodule foreach --quiet 'echo $name'
```
Single repo: skip `submodule foreach`.

### Step 2 — Merge ALL submodules (PARALLEL — monorepo only)

```bash
cd <SUBMODULE_ABSOLUTE_PATH> && git checkout main && git pull && git merge <branch> && git push && git checkout <branch>
```

### Step 3 — Merge parent / root

```bash
git checkout main && git pull && git merge <branch> && git push && git checkout <branch>
```

---

## deploy

Sequential phases: Phase 1 must complete fully before Phase 2.

### Phase 1 — Commit & Push

Execute `push` action.

### Phase 2 — Merge to Main

Execute `merge` action.

If Phase 1 fails, Phase 2 is skipped.

---

## Cautions

- Aborts if ANY repo has merge conflicts
- Submodules BEFORE parent (both in push and merge)
- NEVER use `git add .` — use `git add -A` from the correct directory
- If any push/merge fails, stop and report

## Performance Budget

- **Max Task agents**: 1 per dirty submodule
- **Max Bash calls per agent**: 1 (all commands chained)
- **Max API calls total**: ≤ 10

ULTRATHINK
