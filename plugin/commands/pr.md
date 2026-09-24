---
description: Use when the user runs /mustard:pr or asks to open a pull request, review a colleague's, or merge one. The pull request door — open, review, merge.
argument-hint: <open|review|merge> [<pr-number>] [--confirm]
disable-model-invocation: true
---
# /mustard:pr — the pull request door

Print each JSON answer as it came. The PR body is not yours to write. The binary builds it, and the title, from the spec.

## open

1. The spec closes first: `mustard-rt run close --spec {spec}`. Run it in the background and wait for the notice that it ended: it runs the lint and the whole suite the server runs, which can take longer than the 10 minutes the terminal waits for a command. A refusal says what is missing.
2. Record the summary for whoever reviews, in two or three sentences: `mustard-rt run write pr_summary --spec {spec} --json '{"text":"…"}'`. Every number in it is measured, and what is still open is named.
3. Run `mustard-rt run pr-open --base <base> --head <branch> --spec {spec}`. An existing pull request only gets its body rewritten.
4. It publishes: it does not judge and does not gate. A red suite is reported, never investigated here. A push refused by the repository's own tooling is reported, not routed around.

## review

1. `mustard-rt run pr-review` lists the open pull requests; `--pr <n>` prints the brief of one.
2. Review it with the `mustard-review` agent, from that brief: it returns its text to you and records nothing.
3. Relay the outcome to the user.

## merge — only when the user asks

1. Run `mustard-rt run pr-merge --pr <n>`; a submodule's pull request takes `--root <its folder>`.
2. `confirm` touched nothing: read `reason`. A missing or rejected verdict is a question for the user; on a yes, run it again with `--confirm`. Provider checks still running mean wait; failed ones mean fix; unreadable ones mean check the provider's tooling.
3. `merged`: relay the report and every open pending item it lists.
4. `merge-failed`: the provider refused, and nothing was pruned.
