# /review - Pull Request Review

> Review a PR using Claude's native code-review skill. Auto-detects current branch PR or accepts PR number/URL.

## Trigger

`/review [pr-number-or-url]`

## Configuration

Reads `mustard.json` from the **project root** for `git.provider`.

| Provider | CLI | PR detection |
|----------|-----|--------------|
| `github` | `gh` | `gh pr view --json number,url` |
| `gitlab` | `glab` | `glab mr view` |

## Behavior

- **ZERO confirmations** — detect PR, invoke review, done.
- **ZERO questions** — auto-detect if no argument provided.

---

## Step 1 — Resolve PR

### If argument provided

- Numeric → treat as PR number
- URL → use directly

### If no argument

```bash
gh pr view --json number,url,title,headRefName 2>/dev/null
```

If no PR found for current branch → error:
> No open PR found for current branch. Run `/git merge` first to create one.

---

## Step 2 — Prefetch PR data

```bash
rtk mustard-rt run review-prefetch <pr-ref> --format json
```

`<pr-ref>` is the PR number or URL resolved in Step 1. The binary fetches all PR data natively and returns a JSON object with the following fields:

| Field | Description |
|-------|-------------|
| `title` | PR title |
| `body` | PR description |
| `author` | PR author login |
| `base` | Base branch ref |
| `head` | Head branch ref |
| `additions` | Lines added |
| `deletions` | Lines removed |
| `changedFiles` | Number of changed files |
| `files[]` | Array of changed file objects (path, patch, status) |
| `comments[]` | Inline and general PR comments |
| `reviews[]` | Existing review submissions (author, state, body) |

Use this JSON as the source of truth for all subsequent steps. Do NOT call `gh pr view --json ...` or any other `gh` subcommands to re-fetch data already present in the prefetch output. Only reach for `gh` when the prefetch is unavailable (see fallback below).

**Fallback:** If `mustard-rt run review-prefetch` exits non-zero or is not found, fall back to the previous approach: `gh pr view --json title,body,author,baseRefName,headRefName,additions,deletions,changedFiles 2>/dev/null` + `gh pr diff`.

---

## Step 3 — Emit DORA event (review.start)

Before invoking the review, emit `review.start` to the harness event bus so `/mustard:stats --pr` can compute review-time DORA metrics:

```bash
mustard-rt run emit-event --event review.start --spec "$MUSTARD_SPEC" --payload "spec=$MUSTARD_SPEC" --payload "target=$PR_TARGET"
```

`$PR_TARGET` is the PR number or URL resolved in Step 1. Set `MUSTARD_SPEC` from the most recent active spec if available (best-effort); omit the `--spec`/`spec=` arguments when no active spec is known. The `spec` payload key is what `event-projections`' `pr-metrics` view uses to pair `review.start` with `review.complete`.

## Step 4 — Invoke Code Review

### Diff-First Dispatch

Antes de invocar Skill ou Task, gere o diff via `mustard-rt run diff-context --phase execute --subproject {sub}` e cole o resultado como bloco `## DIFF` no prompt do agente de review. O diff vira a fonte de verdade — o agente lê arquivos só se o diff for ambíguo. Fail-open: se o diff vier vazio, segue com o fluxo normal (Skill code-review com o PR target).

Use the Skill tool to invoke Claude's native code-review:

```
Skill({
  skill: "code-review",
  args: "<pr-number-or-url>"
})
```

If the native `code-review` skill is not available, fall back to local review with the DIFF as the primary block:

```
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Review: PR <number>",
  prompt: "## DIFF\n<output of mustard-rt run diff-context --phase execute --subproject {sub}>\n\n## TASK\nReview the changes in the current branch against $PARENT. Use the DIFF as the source of truth. Only Read source files if the diff is ambiguous or you need surrounding context — record each Read in your return note. Checklist: SOLID, Security, Performance, Patterns, Integration."
})
```

---

## Step 5 — Emit DORA event (review.complete) + Report

After the review returns, emit `review.complete`:

```bash
mustard-rt run emit-event --event review.complete --spec "$MUSTARD_SPEC" --payload "spec=$MUSTARD_SPEC" --payload "target=$PR_TARGET"
```

Then present the review results as returned by the skill/agent.

---

## Step 6 — Tactical Fix Discovery (advisory)

After the verdict is presented (APPROVED or REJECTED), scan the review agent's return for a `## Tactical Fix Candidates` (or `## Candidatos a Tactical Fix`) section. Each entry there is a small adjacent fix the reviewer flagged — by the qualification criteria in `pipeline-config.md § Tactical Fix Discovery` (≤100 LOC, no public contract change, no pending design decision, no new dependency).

For each candidate, the orchestrator (parent context) prints a suggestion line of the form:

```
Tactical fix candidate: <descrição>
Run: /mustard:tactical-fix <parent-spec> "<descrição>"
```

**This step is advisory.** It does NOT block APPROVED, does NOT force a fix-loop, does NOT keep the pipeline open. The user decides whether to create the sub-spec(s) now, later, or never. A REJECTED verdict still routes through the normal fix-loop (see `resume/SKILL.md § Fix Loop Dispatch Protocol`); tactical-fix is for *adjacent* findings, not for the REJECTED root cause.

If the review agent returned no such section, skip this step silently.

---

## Provider Support

| Provider | Auto-detect | Manual URL |
|----------|-------------|------------|
| GitHub | `gh pr view` | yes |
| GitLab | `glab mr view` | yes |
| Bitbucket | no | yes |

---

## Model Selection

**Initial reviews**: use default model per `pipeline-config.md § Models` (sonnet for most; opus for Full + new patterns; etc.).

**Re-reviews**: always dispatch with `model: "sonnet"`, regardless of the initial review's model.

**Rationale**:
- Re-reviews verify a targeted fix to already-reviewed code. Sonnet is capable enough even in complex codebases (see `pipeline-config.md` where Sonnet is default for audit, bugfix, and ≤5-file features).
- For Full + new-pattern features (initial review in Opus), this saves ~$5/re-review without introducing Haiku quality risk.
- Simpler than heuristic decision table: one rule, zero edge cases.

## Rules

- NEVER ask for confirmation before invoking the review
- NEVER attempt both Skill and Task — try Skill first, fall back only if unavailable
- ALWAYS use the PR number or URL directly — do NOT pass branch names to the skill
- If provider CLI is missing, instruct the user to install it; do NOT improvise

## Examples

```bash
/review              # Auto-detect PR for current branch
/review 42           # Review PR #42
/review https://github.com/org/repo/pull/42
```

## Performance Budget

- **Max Bash calls**: 1 (PR detection)
- **Max Skill/Task calls**: 1
- **Max API calls total**: ≤ 4
