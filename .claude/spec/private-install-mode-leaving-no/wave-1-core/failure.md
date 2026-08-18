# Wave failure — RESOLVED in place

Declared 2026-08-18 after the second fix-loop: `apps/cli` review found that
`HARNESS_CLAUDE_OUTPUT` was hand-typed and omitted `plans/` (filled because
Mustard's own settings seed sets `plansDirectory`), `graph/`, and the runtime
scratch at depth — 18 real files carrying the operator's own prompt titles
stayed visible to a client's git.

Resolved by the orchestrator rather than a third fix-loop, because the fix was
one derivation, not a design question:

- the directory half of the list now derives from `ClaudePaths::documented_dirs`,
  the catalog whose own doc says to derive from it; only the client-authored
  directories (`commands`, `skills`, `refs`, `agents`, `.obsidian`) are subtracted,
  and that subtraction is the safe half to get wrong — forgetting an entry there
  hides something of the client's, which the ownership ratchet already refuses;
- the ratchet gained its missing SECOND direction: for every documented harness
  directory, assert a rule exists. It only ever validated the rules that were
  emitted, never noticed one that was absent, which is how this leaked green.

Field-proven in a real repository with the built binary: `.claude/plans/*.md`,
`.claude/graph/`, `<sub>/.claude/.metrics/` and the whole install footprint are
invisible, while the client's own `.claude/commands/their-command.md` and
`CLAUDE.md` still show in `git status`.
