---
id: wave.worktree-isolation-becomes-usable-it.1-carry
---

# wave-1-carry

## Summary

A worktree receives the environment the project declares: small files copied, heavy directories linked, and whatever could not travel is named.

## Network

- Parent: [[spec.worktree-isolation-becomes-usable-it]]

## Tasks

- [ ] Add a `worktree` section to the project config (packages/core/src/domain/config.rs, beside `git`): `carry` — git-ignored paths COPIED into a fresh worktree; `link` — heavy regenerable directories POINTED at the main checkout's copy. Both default to empty, so a project that declares nothing behaves exactly as today.
- [ ] Populate a freshly cut worktree from that declaration in `hook_create` (work_unit_open.rs), in the same place `init_submodules` already runs and with the same posture: the network and the filesystem are forgiving, a failure degrades to a loud warning and never aborts the creation (a non-zero exit there kills the whole EnterWorktree).
- [ ] Copy for `carry`, link for `link` — never the reverse. A linked `.env` would leak edits back into the main checkout; a copied `node_modules` is the ten-minute wait that makes worktrees unusable.
- [ ] Report what did NOT travel: a declared path missing from the main checkout, a copy that failed, a link the platform refused. One list, named, so the operator learns it before hitting it mid-work — never a silent partial environment.
- [ ] Do not touch what already works: submodules keep being populated, and `.claude/` keeps resolving to the main checkout (the redirect is what keeps the harness's state single).

## Files

- `packages/core/src/domain/config.rs`
- `apps/rt/src/commands/work_unit_open.rs`

## Reality Obligations

- **RO-1.1** — Confirm on this Windows install how a directory junction is created WITHOUT elevated privileges, and whether `std::os::windows::fs::symlink_dir` requires Developer Mode or admin rights. The whole `link` verb rests on it being available to an ordinary user; the repository cannot answer this.
