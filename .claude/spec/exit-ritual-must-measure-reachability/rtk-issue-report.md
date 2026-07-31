# rtk issue report — `git log` drops merge commits, silently

> For the **rtk** project (Rust Token Killer). Filed from the `mustard` repo, where the
> filter changed the answer to a question that authorises deleting branches.
> Measured with `rtk 0.34.1` on Windows 11 (git-bash), against a public repository state.

## Symptom

`rtk git log` removes merge commits from the output, so a command that asks git for one
specific commit can come back describing a **different** commit — and a commit range holding
only merges comes back **empty**, indistinguishable from "this range has nothing in it".

## Reproduction (two lines)

`dd095023` is an ordinary `Merge pull request` commit.

```bash
git log --oneline -1 dd095023
rtk git log --oneline -1 dd095023
```

**Expected** — both name the same commit; the wrapper compresses, it does not re-select.

**Obtained**

```
$ git log --oneline -1 dd095023
dd095023 Merge pull request #134 from rubensrpj/dev_harness-ve-toda-branch-trabalho

$ rtk git log --oneline -1 dd095023
6f180eea @ chore(spec): registro do fechamento — QA 12/12, remocao sem sobreviventes
```

The `-1` is applied by git BEFORE the filter runs: git emits the merge, rtk drops it, and rtk
then shows the next commit to fill the line. The caller asked about `dd095023` and was answered
about `6f180eea`.

### The same defect as an empty range

A range whose only commit is a merge:

```bash
git log --oneline dd095023 --not dd095023^1 dd095023^2      # 1 commit, 79 bytes
rtk git log --oneline dd095023 --not dd095023^1 dd095023^2  # empty, 1 byte (a lone "\n")
```

Nothing in the output says a commit was withheld. A caller cannot tell this apart from a range
that is genuinely empty.

### Scale of the filtering, on an ordinary range

```bash
git log --oneline b33d4264~3..b33d4264       # 726 bytes
rtk git log --oneline b33d4264~3..b33d4264   # 549 bytes
```

Passing `--merges` explicitly DOES return the merges (measured), so the filter appears to be
"drop merges unless the user asked for merges". That is a reasonable token-saving heuristic for
a human reading history, and a wrong one for any caller that treats the output as an answer.

### `rev-list` is unaffected

```bash
git rev-list --pretty=oneline --no-commit-header b33d4264~3..b33d4264       # 1014 bytes
rtk git rev-list --pretty=oneline --no-commit-header b33d4264~3..b33d4264   # 1014 bytes
```

Byte-identical — rtk passes `rev-list` straight through.

## Impact

A destructive decision resting on an empty range.

The harness that found this asks "does this branch still hold commits that are not on its base?"
before offering to delete the branch and its remote ref. Read through `rtk git log`, a branch
whose remaining commits are merges answers **nothing** — so the sweep concludes the branch is
fully integrated and offers a deletion that destroys the only reference to those commits.

The dangerous property is not the compression. It is that the compressed answer is
**well-formed and wrong**: no marker, no count, no stderr note. Every downstream check that says
"if the output is empty, then …" inverts.

Same shape, smaller blast radius: any tooling that pipes `git log --oneline <range>` into a
changelog, a release-note generator or a "commits since tag" count silently under-reports.

## Suggested fix (in rtk's own order of preference)

1. Do not drop merge commits when the invocation constrains the selection — an explicit range,
   an explicit SHA, or `-n`/`-<N>`. The user asked for specific commits; compressing them is
   fine, removing them is answering a different question.
2. Failing that, emit a footer when anything was withheld (`[rtk] 1 merge commit hidden`), so an
   empty result is never ambiguous. rtk already appends decoration to other outputs (below), so
   the channel exists.

## Secondary findings — output decoration on machine-parsed commands

Recorded for completeness, not as breakage: the two parsers in the reporting repo skip these
today (one requires a tab, the other filters empty lines). They are noted because a caller that
did not know to skip them would mis-parse, and because they share the root shape with the finding
above — decoration mixed into a data stream.

**1. A `--- Changes ---` banner is appended to `git diff --name-status`.**

```
$ git diff --name-status HEAD~1
M<TAB>plugin/.claude-plugin/plugin.json

$ rtk git diff --name-status HEAD~1
M<TAB>plugin/.claude-plugin/plugin.json
<blank>
--- Changes ---
<blank>
```

A banner AFTER the data, with no path and no status column. A naive line-splitting parser reads
`--- Changes ---` as a fourth file.

**2. An empty `git diff --cached` returns one byte instead of zero.**

```
$ git diff --cached | wc -c
0
$ rtk git diff --cached | wc -c
1        # a lone "\n"
```

`[ -z "$(git diff --cached)" ]` still holds (command substitution strips the trailing newline),
but `wc -c`, a byte-length check, or a `read`-based loop sees content where there is none.

## Environment

| | |
|---|---|
| rtk | 0.34.1 |
| git | via git-bash on Windows 11 Pro (10.0.26200) |
| repo | `rubensrpj/mustard`, branch `dev`, SHAs `dd095023` / `b33d4264` |

All figures above were measured, not estimated; every command in this report is runnable as
written from a clone of that repository at those SHAs.
