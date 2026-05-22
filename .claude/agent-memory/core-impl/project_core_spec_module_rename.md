---
name: core-spec-module-rename
description: mustard-core spec_doc module renamed to spec (2026-05-22), layered on crate::fs; rt/cli/dashboard still import spec_doc and need fixing next pass
metadata:
  type: project
---

`packages/core/src/spec_doc/` was renamed to `packages/core/src/spec/` (2026-05-22). All public names kept (`read_state`, `write_state`, `parse_state`, `serialize_header`, `rewrite_header`, `status_word`, `header_field`, `stage_label`, `outcome_label`, `flags_label`, `header_region_lines`). lib.rs exports `pub mod spec;` + re-exports.

`read_state` / `write_state` now route through `crate::fs` (`fs::read_to_string`, `fs::write_atomic`) instead of `std::fs`; the pure `&str` parse/serialize cores are unchanged. `write_state` keeps its `std::io::Result<()>` signature (maps crate Error → io::Error) for consumer stability.

**Why:** layer spec-doc I/O on the canonical [[core-fs-seam]] while keeping the API stable.

**How to apply:** rt/cli/dashboard still `use mustard_core::spec_doc::…` — they WON'T build until the next pass fixes those imports to `mustard_core::spec::…`. Only `-p mustard-core` builds/tests after step A. Don't be surprised by workspace build failures in other crates mid-migration.
