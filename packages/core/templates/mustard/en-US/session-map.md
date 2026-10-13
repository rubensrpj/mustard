# Mustard in this project

Binary searches and computes; LLM decides and implements.

- Resume with `mustard-rt run resume`. Queries open no spec. Changes: `mustard-rt run open`, goal in `context`; confirm a suggested goal through `goal_approval` from `run resume`/`run grill`. Then suggest `/clear`; the resume line retains state.
- Code: `mcp__mustard__search`, with local intent and investigation purpose; preserve arguments and scope. CLI: `mustard-rt run search --shell-output` with the original search. Bash description: `mustard:spec: <question>`.
- Reuse current excerpts; Read with offset/limit is crossed by the hook without another model. No range: `run map summary --file <file>`. Reread changed sources. References and comments are not runtime proof.
- Investigation: `mustard-rt run knowledge`; missing analysis: `run knowledge --prepare-session claude --query "<question>"`. Delegate only useful bounded investigation; every agent records in the spec through `mustard-rt run write` and returns in two lines. Reuse current analysis.
- Spec: `mustard-rt run read`/`run write`; never edit `spec.*`. Facts: `mustard-rt run answer --facts '<JSON>'`. Measurements: `mustard-rt run evidence --spec <spec>`; attach using `--point <code> --receipt <receipt> --text "<measurement, input and limits>"`. Do not generalize one sample.
- Same subject: `write request`, not a pending item. Record authorized changes; when uncertain, ask once. On a closed spec or with the pull request open, `mustard-rt run reopen --reason "<why>"` comes first, before `write request`. PR the server failed: `mustard-rt run reopen --fix --reason "<why>"`. Another subject uses `mustard-rt run pending --add`; if the user wants it done now, suggest another conversation.
- Unauthorized changes require acceptance; fix Mustard in the product.
- Before commit: `mustard-rt run review-wave --wave <n> --spec <spec>`. Read diff/rules/skills/tests. Accept with REVIEWED and fingerprint, or REJECTED and reason. USAGE does not approve. Replanning waits for acceptance; blocked work must not become the same wave again.
- `/mustard-panel`: project, specs, waves and local usage; CLI `run panel`. `/mustard-pages`, `run publish` and `mustard-rt run page` (markdown): only on request; export is not publishing; `--include-consumption` requires authorization.

Answer in the order of explaining from the response style. Commit/PR omit clients, email and local paths.

## Resuming

`mustard-rt run resume`.
