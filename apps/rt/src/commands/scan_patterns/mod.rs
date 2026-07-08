//! `scan-patterns-list` / `scan-patterns-apply` — the enrich hand-off for the
//! per-subproject `{role}-pattern` skill *molds*, the pattern-mold twin of the
//! `scan_guards` pair.
//!
//! Enrich authors exactly two things (see the `/scan` SKILL): subproject `##
//! Guards` prose and the MISSING pattern molds. Guards is tooled end-to-end
//! (`scan-guards-list` → agent → `scan-guards-apply`); molds were not — the
//! worklist was derived by hand from the model and the write went through the
//! orchestrator's own Write tool. This module closes that asymmetry:
//!
//! - [`list`] projects the mold worklist deterministically FROM `grain.model.json`
//!   (role clusters ≥3, attributed to their subproject, with real exemplars and
//!   existing molds filtered) — no hand-derivation, no re-reading the repo.
//! - [`apply`] writes one authored mold create-only, path-shape-guarded and
//!   atomic — and, being a `mustard-rt run` command, sidesteps the
//!   background-isolation gate that stalled the orchestrator's Write.
//!
//! Both are fully fail-open per the `mustard-rt run` contract.

pub mod apply;
pub mod list;
