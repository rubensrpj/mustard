---
name: w2-enforce-registry-test-break
description: Stale — W2 enforce_registry.rs test compile break was already resolved; `use std::path::Path` exists inside the tests module
metadata:
  type: project
---

RESOLVED. Verified 2026-05-26: `apps/rt/src/hooks/enforce_registry.rs` already has `use std::path::Path;` at line 160 inside `#[cfg(test)] mod tests`. `cargo build -p mustard-rt --tests` and `cargo test -p mustard-rt --no-run` both pass.

**Why:** Originally observed after W2 of `2026-05-26-claude-paths-single-source` removed the top-level `Path` import. A follow-up fix re-introduced it inside the tests block, which is exactly where it was needed.

**How to apply:** No action. Memory kept as breadcrumb so future agents don't re-flag this as broken.
