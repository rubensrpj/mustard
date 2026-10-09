# Mustard in this project

Flow: survey, plan, approval, waves, review, close, PR. Binary: state, search and calculations. Model: reasoning and code.

## Request and survey

- Open changes with `mustard-rt run open`, record the goal in `context`, then suggest `/clear`; the resume line shows the state. Questions, reads and status open no spec. Other branches do not block.
- Explain in the order of explaining from the response style. Check code/history before removing anything.
- Record each answer with `mustard-rt run answer`.
- Search/read code through `mcp__mustard__search`: `{request:{tool,input,intent,purpose,choose?}}`. Preserve arguments and scope. In `intent`, state the specific question this search must answer; for a spec, the overall goal is already in `context`. Explicitly set `purpose`: `locate`, `understand`, `spec`, `implement` or `validate`. Investigation requires intent; literal locate permits an empty intent. `Read` preserves the original result.
- Without the tool: `mustard-rt run search --shell-output --intent "<specific question>" --purpose spec -- rg -n "<pattern>" .`. For native Bash, description `mustard:spec: <question>` carries purpose through the hook. Unannotated searches remain `locate`. Expand incomplete ranges; `--raw` returns native bytes. `choose:true`/`--choose` permits Jev for unresolved responsibility alternatives, without enabling it on every search.
- `mustard-rt run knowledge`: investigation/export.

## Open spec

- On a closed spec or with the pull request open, `mustard-rt run reopen --reason "<why>"` comes first, before `write request`. For a PR the server failed: `mustard-rt run reopen --fix --reason "<why>"`.
- Errors, adjustments and improvements on the same subject join the same spec through `write request`, never pending items. Record and report what is certain; when uncertain, propose and ask once. Another subject uses `mustard-rt run pending --add`; if the user wants it done now, suggest another conversation.
- Changes outside the authorization require the user's yes. A correction to Mustard becomes a product adjustment, not just memory.
- Read the spec through `mustard-rt run read <block>` and record through `write`; never hand-edit `spec.*`.
- Delegate any investigation that opens many files; handle a single check yourself. Ask every agent for findings in the spec through `mustard-rt run write` and a return in two lines.

## Tracking

- `/mustard-panel`: project, specs, execution and local consumption; querying and rendering call neither a model nor Jev.
- `/mustard-pages` requires a request: a dated snapshot. Local export does not confirm publishing; updates require another action.
- Without Mods: `mustard-rt run panel --root <project> --spec <spec>`. Export: `mustard-rt run publish --spec <spec>`; `--include-consumption` authorizes sharing consumption.
- Standalone page: `mustard-rt run page` takes markdown.
- `mustard-rt run spend` measures locally; `--publish` requires a request.

## Resuming

Resume with `mustard-rt run resume`. Commits/PRs omit client names, email and machine paths.
