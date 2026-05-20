//! `model` — pure data types shared across hooks, scripts, and the CLI.
//!
//! Every type in this module is a plain `serde` struct or enum with **no side
//! effects**: no I/O, no filesystem access, no logging. Side-effecting
//! infrastructure lives in the `io` layer (Wave 2).
//!
//! Submodules:
//!
//! - [`event`] — the harness event schema (stored in `mustard.db`).
//! - [`contract`] — the hook contract: [`contract::HookInput`],
//!   [`contract::Verdict`], [`contract::Outcome`], [`contract::Trigger`], and
//!   the [`contract::Check`] / [`contract::Observer`] traits. **Frozen at the
//!   end of Wave 1** — B3/B4 depend on it.
//! - [`pipeline`] — `pipeline-state` types ([`pipeline::PipelineState`],
//!   [`pipeline::Phase`], [`pipeline::Scope`]).
//! - [`provenance`] — the managed-artifact manifest
//!   ([`provenance::ArtifactManifest`], [`provenance::ArtifactRecord`]).

pub mod contract;
pub mod event;
pub mod pipeline;
pub mod provenance;
