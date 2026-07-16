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
//!   (role clusters ≥3, attributed to their subproject, with real exemplars) —
//!   no hand-derivation, no re-reading the repo. Machine-pristine molds come
//!   back as `mode: "refresh"` so every scan re-authors them fresh; hand-edited
//!   or unmarked molds and recorded declines are never re-proposed.
//! - [`apply`] writes one authored mold — create, or `--refresh` over a mold
//!   whose [`provenance`] marker verifies — path-shape-guarded and atomic; and,
//!   being a `mustard-rt run` command, it sidesteps the background-isolation
//!   gate that stalled the orchestrator's Write.
//! - [`decline`] records the agent's justified refusals so a dead candidate
//!   stops burning a dispatch on every scan.
//! - [`provenance`] is the machine-vs-hand line: a SHA-256 marker stamped at
//!   write time; only a mold whose marker verifies is ever overwritten.
//!
//! All fully fail-open per the `mustard-rt run` contract.

pub mod apply;
pub mod decline;
pub mod list;
pub(crate) mod provenance;
