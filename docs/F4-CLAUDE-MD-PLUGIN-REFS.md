# F4 -> F5 handoff: CLAUDE.md ref-file pointers need skill-mediation

> HANDOFF MARKER (not a fix): the seeded orchestrator `CLAUDE.md` still points at plugin-hosted ref files by bare relative path; F5 must convert those pointers to skill-mediated references.

## Detail

`apps/cli/templates/CLAUDE.md` (the orchestrator rules `mustard init` seeds into a
project's `.claude/`) references ref files by bare relative path, e.g.:

- `pipeline-config.md` (and several `pipeline-config.md § ...` section pointers)
- `refs/locating-code.md`
- `refs/canonical-phases.md`
- `refs/git/worktree-isolation.md`

In Mustard 2.0 those files no longer ship under a project's `.claude/`; they live in
the **plugin** (`plugin/pipeline-config.md`, `plugin/refs/...`). And
`${CLAUDE_PLUGIN_ROOT}` does **not** expand inside `CLAUDE.md`, so a bare `refs/...`
path written there no longer resolves from a consumer project.

## What F5 must do

Convert these pointers to **skill-mediated** references: the plugin skills load their
own refs via `${CLAUDE_PLUGIN_ROOT}`, so `CLAUDE.md` should defer to the relevant
skill (e.g. `pipeline-execution`) rather than naming ref paths directly.

F4 intentionally left `CLAUDE.md`'s routing prose untouched (that rewrite is F5's
job); this file only records the dependency so it is not lost.
