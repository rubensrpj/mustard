//! Dashboard Tauri commands, grouped by domain.
//!
//! Wave 3 of `mustard-unification` introduced this directory so the spec
//! sidecar reader (`commands::specs::read_spec_meta`) can sit next to other
//! spec-scoped commands as the dashboard grows. Existing flat-file commands
//! in `lib.rs` (`dashboard_*`) stay where they are — surgical edits only.

pub mod settings;
pub mod specs;
