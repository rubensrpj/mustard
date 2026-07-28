---
name: checklist-dropped-modelled-dropped-option-string-wave3
description: Checklist "dropped" is modelled as `dropped: Option<String>` (the reason IS the state) rather than a bool plus a reason field — a boolean would let a drop be recorded mutely, which is exactly the forgotten-vs-decided ambiguity it exists to remove.
spec: make-harness-stop-asserting-what
wave: 3
role: general-purpose
session: 9726b360-8b0a-4758-b5c1-c6fa2eb099c7
recorded: 2026-07-28T01:11:21.734Z
source: wave-close
---

Checklist "dropped" is modelled as `dropped: Option<String>` (the reason IS the state) rather than a bool plus a reason field — a boolean would let a drop be recorded mutely, which is exactly the forgotten-vs-decided ambiguity it exists to remove.
