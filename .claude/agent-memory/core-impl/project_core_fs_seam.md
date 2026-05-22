---
name: core-fs-seam
description: packages/core/src/fs/ is the canonical std::fs seam for the whole monorepo; rt/cli/dashboard migrate onto it in later passes
metadata:
  type: project
---

`packages/core/src/fs/` is THE single `std::fs` call site for the monorepo (step A of a workspace-wide consolidation, done 2026-05-22).

- `fs/mod.rs` — `Fs` trait (port), `DirEntry`, module-level free fns (`fs::read_to_string`, `fs::write_atomic`, `fs::read`, `fs::append_line`, `fs::exists`, `fs::read_dir`, `fs::create_dir_all`, `fs::remove_file`, `fs::modified`) backed by a const `RealFs`, plus `fs::real() -> &'static dyn Fs`.
- `fs/real.rs` — `RealFs` (the ONLY `std::fs` usage in core; `map_io` keeps NotFound distinct from Io).
- `fs/memory.rs` — `FakeFs` in-memory test double (RwLock<BTreeMap>/<BTreeSet>), `.seed()` helper.
- `store/fs.rs` is now a thin re-export shim (`pub use crate::fs::{...}`) so old `crate::store::fs::X` / `mustard_core::store::fs::X` paths still resolve.

**Why:** centralize fail-open + atomic-write policy + a future path-guard hook in one place; enable filesystem-free unit tests (DIP).

**How to apply:** Use the free fns for the ~700 mechanical `std::fs::X` → `mustard_core::fs::X` swaps (no dependency threading). Take `&dyn Fs` only on hot/logic-heavy paths that need FakeFs injection. NEXT PASSES (separate): migrate rt, cli, dashboard — they still reference `mustard_core::spec_doc::…` (now renamed) and their own `std::fs`. See [[core-spec-module-rename]].
