---
description: Use when the user runs /mustard:pr or asks to open a pull request or merge one.
argument-hint: <open|merge> [<pr-number>] [--confirm]
disable-model-invocation: true
---
# /mustard:pr — the pull request door

Relay each answer in the response style. The PR body is not yours to write. The binary builds it, and the title, from the spec.

## open

1. The spec closes first: `mustard-rt run close --spec {spec}`. Run it in the background and wait for its end notice: it runs the lint and the whole suite the server runs, which can pass the 10 minutes the terminal waits for a command.
2. Record the summary for whoever reviews, in two or three sentences: `mustard-rt run write pr_summary --spec {spec} --json '{"text":"…"}'`. Every number in it is measured, and what is still open is named.
3. Run `mustard-rt run pr-open --base <base> --head <branch> --spec {spec}`. An existing pull request only gets its body rewritten. Relay `checks.text`: the provider's pipeline right after.
4. It publishes: it does not judge and does not gate. A red suite is reported, never investigated here. A push refused by the repository's own tooling is reported, not routed around.

## merge — only when the user asks

1. Run `mustard-rt run pr-merge --pr <n>`; a submodule's pull request takes `--root <its folder>`.
2. `confirm` touched nothing: read `reason`. A missing or rejected verdict is a question for the user; on a yes, run it again with `--confirm`. Provider checks still running mean wait; failed ones mean fix; unreadable ones mean check the provider's tooling.
3. `merged`: relay the report and every open pending item it lists.
4. `merge-failed`: the provider refused, and nothing was pruned.
