## Branch: dev_rubens
## Unstaged Changes
```
packages/core/.claude/.cluster-cache.json   |  2 +-
 packages/core/tests/.parity.rs.pending-snap | 10 ++++++++++
 2 files changed, 11 insertions(+), 1 deletion(-)
```
## Commits since main
- 38cbcca refactor(core): absorb mustard-specsdb + rename io→store (12/12 ACs)
- 39ecc53 feat(amend): session-bound amendments — passive post-CLOSE capture (16/16 ACs)
- c88c99a feat(followup/rt+core): migrate complete-spec to events (close the loop)
- 8fb250d feat(wave-6a/core): SQLite schema + FTS5 for knowledge/memory
- da0255e feat(wave-2/rt): pipeline_state_for_spec projection
- 4f4d25b feat(wave-1/rt+core): event constants + emit-pipeline subcommand
- 1da3a85 chore(dev_rubens): consolidate workspace state — completed specs + new agents/skills
- 521eaf7 feat(b6): eliminate bun — SQLite event store, Rust MCP face, TS island removal
- 7c0d34c feat(b3): port enforcement hooks to mustard-rt (waves 0-3)
### Changed files since divergence
```
packages/core/.claude/.cluster-cache.json          |    1 +
 packages/core/.claude/commands/exports.md          |  127 ++
 packages/core/.claude/commands/guards.md           |   43 +
 packages/core/.claude/commands/notes.md            |    9 +
 packages/core/.claude/commands/patterns.md         |   92 ++
 packages/core/.claude/commands/recipes.md          |   70 ++
 packages/core/.claude/commands/stack.md            |   53 +
 .../.claude/skills/core-fail-open-error/SKILL.md   |   27 +
 .../core-fail-open-error/references/examples.md    |   96 ++
 .../skills/core-lenient-serde-model/SKILL.md       |   26 +
 .../references/examples.md                         |   68 ++
 .../.claude/skills/core-trait-backed-io/SKILL.md   |   27 +
 .../core-trait-backed-io/references/examples.md    |  109 ++
 packages/core/CLAUDE.md                            |   64 +
 packages/core/Cargo.toml                           |   33 +
 packages/core/src/config.rs                        |  404 +++++++
 packages/core/src/env.rs                           |  488 ++++++++
 packages/core/src/error.rs                         |  167 +++
 packages/core/src/knowledge.rs                     |  506 ++++++++
 packages/core/src/lib.rs                           |   52 +
 packages/core/src/metrics.rs                       |  231 ++++
 packages/core/src/model/contract.rs                |  389 ++++++
 packages/core/src/model/event.rs                   |  406 +++++++
 packages/core/src/model/mod.rs                     |   37 +
 packages/core/src/model/pipeline.rs                |  178 +++
 packages/core/src/model/provenance.rs              |  255 ++++
 packages/core/src/model/view/filter.rs             |  100 ++
 packages/core/src/model/view/mod.rs                |  153 +++
 packages/core/src/model/view/quality.rs            |  119 ++
 packages/core/src/model/view/spec.rs               |  269 +++++
 packages/core/src/model/view/timeline.rs           |  102 ++
 packages/core/src/model/view/wave.rs          
...truncated
