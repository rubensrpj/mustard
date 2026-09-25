# Mustard in this project

Mustard runs every piece of work that changes a file through one flow: survey, plan, approval, waves, review, close and pull request. Every command answers what the next step is. Follow that answer instead of choosing the order yourself.

## When a request arrives

- A request that changes a file opens a spec: run `mustard-rt run open`. The answer brings the question about the base, the type and the branch name.
- A question, a read or a status check opens no spec. Answer it directly.
- On a branch Mustard did not open, it blocks nothing.

## During the survey

- Present one point at a time, in the order of explaining from the response style.
- Check the code before stating a fact.
- Record each answer right away, with `mustard-rt run write <type>`.

## While the spec is open

- A new request from the user joins the same spec, with `write request`; on a closed spec, or one with its pull request open, `mustard-rt run reopen --reason "<why>"` comes first. A pull request the server failed goes to `mustard-rt run reopen --fix --reason "<why>"`. A different subject becomes a pending item, with `mustard-rt run pending --add`.
- A change that comes from you or from an agent only goes ahead with the user's "yes".
- Never edit the `spec.*` files by hand. Record through `write` and read one block with `mustard-rt run read <block>`.
- Hand to an agent any investigation that opens many files. A single check you do yourself.

## Pages

- Never write HTML. A standalone page comes from `mustard-rt run page`, written in markdown.
- Publish on claude.ai only when a command says so, and record the address the way its answer says. The link lives in the status line; do not repeat it in the conversation.

## Commit and pull request

The binary builds the commit message and the pull request body. Never write "Claude", a claude.ai link, an e-mail or a machine path in them.

## Resuming

"Where did I stop" and "let's continue" call for `mustard-rt run resume`. After `/clear`, the line below already says where the spec stands.

How to answer lives in the response style.
