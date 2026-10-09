# Mustard in this project

Binary searches/calculates; LLM implements.

## Survey

- Open changes with `mustard-rt run open`, record the goal in `context`, suggest `/clear`; the resume line shows the state. Queries open no spec; other branches do not block.
- Explain in the order of explaining from the response style. Check code/history before removing. Record answers with `mustard-rt run answer`.
- Search/read through `mcp__mustard__search`: `{request:{tool,input,intent,purpose,choose?}}`. Preserve arguments/scope. `intent`: this search's question. `purpose`: `locate`, `understand`, `spec`, `implement`, `validate`. Only `locate` permits empty intent.
- CLI: `mustard-rt run search --shell-output --intent "<question>" --purpose spec -- rg -n "<pattern>" .`. Bash: `mustard:spec: <question>`; unannotated: `locate`. `--raw`: native. `choose:true`/`--choose`: Jev for ambiguous responsibility.
- Reuse complete bodies; after finding a file, investigate the declaration. Without ranges: `run map summary --file <file>`, then `Read` with `offset`/`limit`. Expand missing ranges; changed source: reread.
- `mustard-rt run knowledge`: investigation.

## Open spec

- Read/write the spec through `mustard-rt run read`/`run write`; never edit `spec.*` by hand.

- On a closed spec or with the pull request open, `mustard-rt run reopen --reason "<why>"` comes first, before `write request`. PR the server failed: `mustard-rt run reopen --fix --reason "<why>"`.
- Errors, adjustments and improvements on the same subject join the same spec through `write request`, never pending items. Record and report what is certain; when uncertain, propose and ask once. Another subject uses `mustard-rt run pending --add`; if the user wants it done now, suggest another conversation.
- Changes outside the authorization require the user's yes. A correction to Mustard becomes a product adjustment, not just memory.
- Delegate any investigation that opens many files. Ask every agent for findings in the spec through `mustard-rt run write` and a return in two lines.

## Tracking

- `/mustard-panel`: project/specs/execution/consumption, no AI; CLI: `run panel`.
- `/mustard-pages` only on request; Local export does not confirm publishing. Update only on request. `run publish --spec <spec>`; `--include-consumption`: permits consumption. `mustard-rt run page`: markdown.

## Resuming

`mustard-rt run resume`. Commit/PR omit clients, email and local paths.
