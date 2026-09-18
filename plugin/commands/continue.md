---
description: Use when the user runs /mustard:continue or asks to pick a spec back up where it stopped. The reserve button — resuming already happens on its own at the start of a session.
argument-hint: [spec-name]
---
# /mustard:continue — pick the spec back up

1. Run `mustard-rt run resume --spec {spec}`. Without a name, drop the flag: the command takes the spec of the checkout's branch.
2. Relay `next` to the user and run `command` when the answer has one. Never decide the next step yourself: the phase decides it.
3. The names it can hand you: `mustard-rt run grill`, `mustard-rt run plan`, `mustard-rt run round` and `mustard-rt run pr-open`.
4. A spec in the plan phase still waits for the approval question, after its page is published. Typing this command approves nothing.
5. The answer carries no page link: the link lives in the status line.

Only on the user's word: `mustard-rt run reopen --reason "<why>"` takes the spec back to the survey, and `mustard-rt run discard` drops it, in two calls with a code. To look without moving: `mustard-rt run read <block>`. A missing or diverging index: `mustard-rt run index`.
