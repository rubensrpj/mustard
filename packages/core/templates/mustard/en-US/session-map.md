# Mustard in this project

All work that changes a file follows one flow: survey, plan, approval, waves, review, close and pull request. Every command says the next step: follow it, not an order of your own.

## When a request arrives

- A request that changes a file opens a spec: run `mustard-rt run open`. Once the goal is recorded (the first `context`), suggest `/clear`: the resume line says where the spec stands.
- A question, a read or a status check opens no spec; answer directly.
- On a branch Mustard did not open, nothing blocks.

## During the survey

- Present one point at a time, in the order of explaining from the response style.
- Check the code and the git history before stating or proposing: what looks dead may have a user, and what left may have left on purpose.
- Record each answer at once with `mustard-rt run answer`.

## While the spec is open

- On a closed spec or with its pull request open, `mustard-rt run reopen --reason "<why>"` comes first, then `write request`. A pull request the server failed: `mustard-rt run reopen --fix --reason "<why>"`. An error, a critical point, an improvement or an adjustment on the same subject joins the same spec through `write request`: it never becomes a pending item. You are the one who tells: when sure, record it and say so; when in doubt, suggest it and ask once. Only a different subject becomes a pending item, with `mustard-rt run pending --add`; if the user wants it done now, suggest another conversation.
- Any other change from you or an agent only goes ahead with the user's "yes".
- Every correction to how Mustard works becomes an adjustment to Mustard itself, never only your memory.
- Never edit `spec.*` by hand: use `write` and `mustard-rt run read <block>`.
- Hand to an agent any investigation that opens many files; a single check is yours. Ask every agent to record in the spec through `mustard-rt run write` and come back in two lines.

## Pages

- Never write HTML. A standalone page comes from `mustard-rt run page`, written in markdown.
- Publish on claude.ai only when a command says so and record the address as it says. The link lives in the status line: do not repeat it in the conversation.

## Commit and pull request

The binary builds the commit message and the pull request body. Never write "Claude", a claude.ai link, an e-mail or a machine path in them.

## Resuming

"Where did I stop" and "let's continue" call for `mustard-rt run resume`.
