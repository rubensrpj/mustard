## Branch: main
## Unstaged Changes
```
.claude/entity-registry.json              |   2 +-
 package.json                              |   6 +-
 pnpm-lock.yaml                            | 868 ++++++++++++++++++++++++++++++
 src-tauri/Cargo.lock                      | 161 +++++-
 src-tauri/Cargo.toml                      |   2 +
 src-tauri/gen/schemas/acl-manifests.json  |   2 +-
 src-tauri/gen/schemas/capabilities.json   |   2 +-
 src-tauri/gen/schemas/desktop-schema.json |  66 +++
 src-tauri/gen/schemas/windows-schema.json |  66 +++
 src-tauri/src/db.rs                       | 220 ++++++--
 src-tauri/src/lib.rs                      | 597 ++++++++++++++++++--
 src/App.tsx                               |  30 ++
 src/components/AggregateOverview.tsx      |  28 +-
 src/components/CommandPalette.tsx         |   8 +-
 src/components/SpecsList.tsx              |  12 +-
 src/components/layout/Sidebar.tsx         |  15 +-
 src/components/layout/Topbar.tsx          |  44 +-
 src/hooks/useActivityFeed.ts              |   4 +-
 src/lib/dashboard.ts                      |  98 ++++
 src/pages/Activity.tsx                    |  57 +-
 src/pages/Home.tsx                        |  51 +-
 src/pages/Knowledge.tsx                   |  20 +-
 src/pages/ProjectDetail.tsx               | 140 +++--
 src/pages/Settings.tsx                    |   6 +-
 src/pages/SpecDetail.tsx                  | 177 +-----
 25 files changed, 2333 insertions(+), 349 deletions(-)
```
## Untracked Files (26 total)
- .claude/memory/decisions.json
- .claude/spec/active/2026-05-13-dashboard-commands-catalog/spec.md
- .claude/spec/completed/2026-05-13-dashboard-content-richness/spec.md
- .claude/spec/completed/2026-05-13-dashboard-live-pipeline-status/spec.md
- .claude/spec/completed/2026-05-13-dashboard-telemetry-tab/spec.md
- .claude/spec/completed/2026-05-13-dashboard-type-scale/spec.md
- .playwright-mcp/page-2026-05-13T02-45-27-158Z.yml
- .playwright-mcp/page-2026-05-13T02-46-04-995Z.yml
- .playwright-mcp/page-2026-05-13T02-46-13-572Z.yml
- .playwright-mcp/page-2026-05-13T02-47-37-504Z.yml
- ...and 16 more
