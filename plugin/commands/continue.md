---
description: Use when the user runs /mustard:continue or asks to resume a spec.
argument-hint: [spec-name]
---
# /mustard:continue — pick the spec back up

1. Run `mustard-rt run resume --spec {spec}`. Without a name, drop the flag: the command takes the spec of the checkout's branch.
2. Relay `next` and `checks.text`, and run `command`, when present. Never decide the next step yourself: follow the phase.
3. The names it can hand you: `mustard-rt run grill`, `mustard-rt run plan`, `mustard-rt run round --dispatch` or `mustard-rt run pr-open`. Bare `run round` only reads state.
4. A spec in the plan phase still waits for the exact approval question; typing this command approves nothing.

Only on the user's word: `mustard-rt run reopen --reason "<why>"` reopens the spec, and `mustard-rt run discard` drops it, in two calls with a code. To look without moving: `mustard-rt run read <block>`. A missing or diverging index: `mustard-rt run index`.
