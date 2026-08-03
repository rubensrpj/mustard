---
name: kept-scan-plugin-commands-with-user-wave6
description: Kept scan.md in plugin/commands/ with `user-invocable: false` rather than deleting it or moving it to refs/ — `scan_clean_gate` is a PreToolUse(Skill) gate keyed on `Skill(mustard:scan)`, so removing the command file silently makes the clean-tree precondition inert for the router-dispatched full pass.
spec: work-unit-lives-on-its
wave: 6
role: general-purpose
session: 12c0e429-f254-41a9-8a8a-c41d3df589d0
recorded: 2026-08-03T09:46:21.764Z
source: wave-close
---

Kept scan.md in plugin/commands/ with `user-invocable: false` rather than deleting it or moving it to refs/ — `scan_clean_gate` is a PreToolUse(Skill) gate keyed on `Skill(mustard:scan)`, so removing the command file silently makes the clean-tree precondition inert for the router-dispatched full pass.
