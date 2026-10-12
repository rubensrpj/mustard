# Mustard in this project

Binary searches; LLM implements.

## Survey

- Open changes with `mustard-rt run open`, record the goal in `context`, suggest `/clear`; the resume line shows the state. Queries open no spec.
- Answer in the order of explaining from the response style; check code/history before removing. Use `mustard-rt run answer --facts '<JSON>'`.
- Search through `mcp__mustard__search`: `{request:{tool,input,intent,purpose,choose?}}`; preserve arguments/scope. `intent`: resource + operation/constraints, or 2–8 `- question` bullets. `purpose`: `locate`, `understand`, `spec`, `implement`, `validate`.
- CLI: `mustard-rt run search --shell-output --intent "<question>" --purpose spec -- rg -n "<pattern>" .`. Bash: `mustard:spec: <question>`. `--choose`: optional Jev.
- Reuse current excerpts; `Read` with `offset`/`limit` crosses evidence without AI. No range: `run map summary --file <file>`. Reread changed sources; references do not prove behavior.
- `mustard-rt run knowledge`: investigation.

## Open spec

- Spec: `mustard-rt run read`/`run write`; never edit `spec.*`.

- On a closed spec or with the pull request open, `mustard-rt run reopen --reason "<why>"` comes first, before `write request`. PR the server failed: `mustard-rt run reopen --fix --reason "<why>"`.
- Same subject: `write request`, not a pending item. Record authorized changes; when uncertain, ask once. Another subject uses `mustard-rt run pending --add`; if the user wants it done now, suggest another conversation.
- Unauthorized changes require acceptance; fix Mustard in the product.
- Delegate any investigation that opens many files; every agent records in the spec through `mustard-rt run write` and returns in two lines.

## Tracking

- `/mustard-panel`: local; CLI: `run panel`.
- `/mustard-pages` only on request; export is not publishing. `run publish --spec <spec>`; `--include-consumption`: permits consumption. `mustard-rt run page`: markdown.

## Resuming

`mustard-rt run resume`. Commit/PR omit clients, email and local paths.

## Wave review

Before commit: `mustard-rt run review-wave --wave <n> --spec <spec>`. Check diff/rules/skills/tests. Accept: `<REVIEWED>{"wave":1,"fingerprint":"<receipt>","note":"<conclusion>"}</REVIEWED>`; reject with `REJECTED`. `USAGE` does not approve. Changes invalidate review. Revise blocked tasks before dispatch; replan awaits the user.
