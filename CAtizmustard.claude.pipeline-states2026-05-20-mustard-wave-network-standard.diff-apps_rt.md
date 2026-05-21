## Branch: dev_rubens
## Unstaged Changes
```
apps/rt/.claude/.agent-state/main-context.counter.json | 2 +-
 apps/rt/.claude/.cluster-cache.json                    | 2 +-
 2 files changed, 2 insertions(+), 2 deletions(-)
```
## Commits since main
- 38cbcca refactor(core): absorb mustard-specsdb + rename io→store (12/12 ACs)
- 39ecc53 feat(amend): session-bound amendments — passive post-CLOSE capture (16/16 ACs)
- c88c99a feat(followup/rt+core): migrate complete-spec to events (close the loop)
- 4867d6a fix(qa-loop): clean up AC-8/AC-9 substring matches + amend AC-13
- 3605487 fix(review-loop/rt): clean up Wave 6b/5 review CRITICAL+WARNINGs
- 33e59d3 feat(wave-5/rt): pipeline-state ingest + docs-audit entry
- d70f25f feat(wave-6b/rt): memory/knowledge writers + ingest -> SQLite
- 689c155 feat(wave-3a/rt): migrate hook readers to pipeline_state_for_spec
- da0255e feat(wave-2/rt): pipeline_state_for_spec projection
- 4f4d25b feat(wave-1/rt+core): event constants + emit-pipeline subcommand
- 1da3a85 chore(dev_rubens): consolidate workspace state — completed specs + new agents/skills
- 58a303b feat(wave-1/rt): apply_vendored fetches upstream via shallow git clone
- 521eaf7 feat(b6): eliminate bun — SQLite event store, Rust MCP face, TS island removal
### Changed files since divergence
```
.../.claude/.agent-state/main-context.counter.json |    1 +
 apps/rt/.claude/.cluster-cache.json                |    1 +
 apps/rt/.claude/commands/guards.md                 |   48 +
 apps/rt/.claude/commands/modules.md                |   90 +
 apps/rt/.claude/commands/notes.md                  |    9 +
 apps/rt/.claude/commands/patterns.md               |   90 +
 apps/rt/.claude/commands/recipes.md                |   90 +
 apps/rt/.claude/commands/stack.md                  |   39 +
 .../skills/rt-fail-open-dispatch-pattern/SKILL.md  |   31 +
 .../references/examples.md                         |   77 +
 .../.claude/skills/rt-hook-module-pattern/SKILL.md |   36 +
 .../rt-hook-module-pattern/references/examples.md  |   72 +
 .../skills/rt-run-subcommand-pattern/SKILL.md      |   35 +
 .../references/examples.md                         |   80 +
 apps/rt/CLAUDE.md                                  |   83 +
 apps/rt/Cargo.toml                                 |   57 +
 apps/rt/src/dispatch.rs                            |  153 ++
 apps/rt/src/hooks/amend_capture.rs                 |  807 ++++++++
 apps/rt/src/hooks/bash_guard.rs                    | 2130 ++++++++++++++++++++
 apps/rt/src/hooks/budget.rs                        |  828 ++++++++
 apps/rt/src/hooks/close_gate.rs                    | 1534 ++++++++++++++
 apps/rt/src/hooks/enforce_registry.rs              |  301 +++
 apps/rt/src/hooks/knowledge.rs                     | 1026 ++++++++++
 apps/rt/src/hooks/mod.rs                           |   39 +
 apps/rt/src/hooks/model_routing.rs                 |  697 +++++++
 apps/rt/src/hooks/path_guard.rs                    |  946 +++++++++
 apps/rt/src/hooks
...truncated
