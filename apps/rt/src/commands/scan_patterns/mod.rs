//! `scan-patterns-*` — the enrich hand-off for the per-subproject
//! `{role}-pattern` skill *molds*, the pattern-mold twin of the `scan_guards`
//! pair.
//!
//! A mustard-generated mold is DERIVED: it is regenerated fresh on every scan,
//! never preserved. The mold flow is **sweep → list → author → apply**:
//!
//! - [`sweep`] deletes every mustard-generated mold (`source: scan`) BEFORE
//!   generation, so each is re-authored from the current exemplars with no bias
//!   from its old text. A hand-authored/adopted mold (`source: manual`) is
//!   preserved.
//! - [`list`] projects the mold worklist deterministically FROM
//!   `grain.model.json` (role clusters ≥3, attributed to their subproject, with
//!   real exemplars). Post-sweep everything is a create; a surviving hand mold
//!   or a recorded decline is never proposed.
//! - [`apply`] writes one authored mold create-only, path-shape-guarded and
//!   atomic, stamping the [`origin`] notice; being a `mustard-rt run` command it
//!   sidesteps the background-isolation gate that stalled the orchestrator's
//!   Write.
//! - [`decline`] records the agent's justified refusals so a dead candidate
//!   stops burning a dispatch on every scan.
//! - [`origin`] is the mustard-vs-hand line: the frontmatter `source:` field.
//!
//! All fully fail-open per the `mustard-rt run` contract.

pub mod apply;
pub mod decline;
pub mod list;
pub(crate) mod origin;
pub mod sweep;
