# Mustard in this project

- Queries open no spec. Changes: `mustard-rt run open`; then record the goal with `mustard-rt run write context`, `origin` on the user's message; it is approved together with the plan. Then suggest `/clear`; the resume line keeps state.
- Code: `mcp__mustard__search`, with local intent and purpose; preserve arguments and scope. CLI: `mustard-rt run search --shell-output` with the original search. Bash description: `mustard:spec: <question>`.
- Reuse current excerpts; the hook crosses Read with offset/limit. No range: `run map summary --file <file>`. Reread changed sources. Comments are not runtime proof.
- Dossier: `mustard-rt run knowledge`; else `run knowledge --prepare-session claude --query "<question>"`. Delegate only useful bounded investigation; wave agent: `write delivered`, errors in `leftovers`, two lines back; the hook refuses it `write request`/`task`: the conductor records.
- Spec: `mustard-rt run read`/`run write`; find with `mustard-rt run find "<text>"`. Facts: `mustard-rt run answer --facts '<JSON>'`. Long JSON: Write a temp file and pass `--json @file` (or `--facts`), no script. Measures: `mustard-rt run evidence --spec <spec>`.
- Same subject: `write request`, not a pending item. Record authorized changes; when uncertain, ask once. With a closed spec or pull request open, `mustard-rt run reopen --reason "<why>"` comes first, before `write request`. PR the server failed: `mustard-rt run reopen --fix --reason "<why>"`. Another subject uses `mustard-rt run pending --add`; if one wants it done now, suggest another conversation. Backlog: `mustard-rt run triage`.
- Before commit: `mustard-rt run review-wave --wave <n> --spec <spec>` sums up; `--diff` adds it. Accept with `<REVIEWED>{"wave":<n>,"note":"…"}</REVIEWED>` or REJECTED and reason. Blocked work never becomes the same wave.
- `/mustard:mods`: local project and usage. `/mustard:pages`, `run publish` and `mustard-rt run page` (markdown): only on request; export is not publishing; `--include-consumption` requires authorization.

Commit/PR omit clients, email and local paths.

## Resuming

`mustard-rt run resume`.
