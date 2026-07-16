---
description: Use when the user runs /review or asks to review, code-review, or audit a pull request. Auto-detects current branch PR or accepts PR number/URL.
source: manual
---
<!-- mustard:generated -->
# /review — Pull Request Review

> ZERO confirmations, ZERO questions — detect PR, invoke review, done.

`/review [pr-number-or-url]` — reads `mustard.json#git.provider` (`github`/`gitlab`).

## 1. Resolve + prefetch

Numeric arg = number, URL = used directly. No arg: `gh pr view --json number,url,title,headRefName`. No PR → *"No open PR found for current branch. Run `/git merge` first."*

```bash
rtk mustard-rt run review-prefetch <pr-ref> --format json
mustard-rt run diff-context --phase execute --subproject {sub}
```

Prefetch returns `title`/`body`/`author`/`base`/`head`/`additions`/`deletions`/`changedFiles`/`files[]`/`comments[]`/`reviews[]` — source of truth, do NOT re-fetch. Fallback: `gh pr view --json …` + `gh pr diff`.

## 2. Emit + invoke

`mustard-rt run emit-event --event review.start --spec "$MUSTARD_SPEC" --payload "spec=$MUSTARD_SPEC" --payload "target=$PR_TARGET"` → paste the diff as a `## DIFF` block → `Skill({ skill: "code-review", args: "<pr-ref>" })`. Fallback (skill unavailable): `Task(general-purpose)` with the DIFF as source of truth (reads source only when ambiguous). Checklist: SOLID, Security, Performance, Patterns, Integration.

## 3. Emit complete + report

`mustard-rt run emit-event --event review.complete --spec "$MUSTARD_SPEC" --payload "spec=$MUSTARD_SPEC" --payload "target=$PR_TARGET"` → present results verbatim.

## 4. Emit the verdict (required — the resume gate reads this)

Once the fix-loop settles, record it so `resume-bootstrap`'s post-execute gate advances past REVIEW (with no `review.result` the spec stays at `ReviewPending`):

```bash
mustard-rt run review-result --spec "$MUSTARD_SPEC" --verdict <approved|rejected> --critical <N> [--subproject {sub}]
```

`<N>` = count of critical findings (0 when `approved`). Emit `rejected` honestly when the fix-loop did not clear the blocking findings — never record `approved` to unblock.

## 5. Tactical-fix discovery (detect + propose, never auto-create)

Scan the return for `## Tactical Fix Candidates` / `## Candidatos a Tactical Fix`; per entry print *"Tactical fix candidate: <desc>\nRun: /mustard:tactical-fix <parent> \"<desc>\""*. Doesn't block APPROVED; REJECTED still routes through the normal fix-loop. Qualification → `${CLAUDE_PLUGIN_ROOT}/pipeline-config.md § Tactical Fix Discovery`. Include a `tactical_fix_candidates` array in the `review.result` payload (each `{description (required), scope?, severity?}`) so `mustard-rt run tactical-fix-detect --spec <spec>` proposes each (idempotent `tactical_fix.proposed`; never scaffolds — creation stays a one-confirmation step).

## Inviolable

- NEVER confirm before invoking. Skill first, Task only as fallback — never both.
- ALWAYS pass a PR number/URL — never branch names.
- Budget: ≤1 Bash for PR detection, ≤1 Skill/Task call, ≤4 API calls total.
