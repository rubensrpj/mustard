---
id: wave.field-report-fix-package-sialia.5-boundary
---

# wave-5-boundary

## Summary

the git-boundary fact (subproject is its own git repo) flows census -> dispatch items -> rendered prompts -> branch-gate base

## Network

- Parent: [[spec.field-report-fix-package-sialia]]
- Depends on: [[wave.field-report-fix-package-sialia.2-agents]]

## Tasks

- [ ] In packages/core/src/domain/scan.rs add own_git_root: bool with #[serde(default)] to Project (grain.model.json back-compat); expose the existing git-root check (`.git` dir OR file) from packages/core/src/io/workspace.rs as a reusable helper instead of duplicating it
- [ ] In apps/rt/src/commands/scan.rs set the flag per subproject when writing the census
- [ ] Thread own_git_root (serde default false) onto DispatchItem (dispatch_plan.rs) and AdvanceItem (wave_advance.rs) from the census project entry matching the item's subproject
- [ ] In the agent prompt render (apps/rt/src/commands/agent/render/mod.rs), when the target subproject carries the flag, append a fixed technical block to the rendered prompt: this subproject is its own git repository — separate commit history; commit inside the subproject only; NEVER bump the superproject gitlink pointer
- [ ] In work_branch_gate.rs: detect a nested git root between the state root and the edited file (walk up to the first `.git` dir-or-file); when found, resolve the integration base from that repo's own default branch (`git symbolic-ref refs/remotes/origin/HEAD`, fallback to its current branch) so the {base}_{slug} name uses the SUBMODULE's base, never the superproject's; fail-open to today's behavior when detection fails
- [ ] Unit tests named with `own_git_root`: census flags a fixture dir containing a `.git` FILE; DispatchItem/AdvanceItem carry the flag; rendered prompt contains the gitlink sentence; branch gate resolves the nested base

## Files

- `packages/core/src/domain/scan.rs`
- `packages/core/src/io/workspace.rs`
- `apps/rt/src/commands/scan.rs`
- `apps/rt/src/commands/pipeline/dispatch_plan.rs`
- `apps/rt/src/commands/pipeline/wave_advance.rs`
- `apps/rt/src/commands/agent/render/mod.rs`
- `apps/rt/src/hooks/write/work_branch_gate.rs`
