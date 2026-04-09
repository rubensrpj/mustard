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

## Step 2 — Invoke Code Review

Use the Skill tool to invoke Claude's native code-review:

```
Skill({
  skill: "code-review",
  args: "<pr-number-or-url>"
})
```

If the native `code-review` skill is not available, fall back to local review:

```
Task({
  subagent_type: "general-purpose",
  model: "opus",
  description: "Review: PR <number>",
  prompt: "Review the changes in the current branch against $PARENT. Checklist: SOLID, Security, Performance, Patterns, Integration."
})
```

---

## Step 3 — Report

Present the review results as returned by the skill/agent.

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
