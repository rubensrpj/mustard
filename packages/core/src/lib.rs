#![forbid(unsafe_code)]
// `clippy::unwrap_used` is `deny` workspace-wide so no hook-path code can
// panic (b2 spec § Preocupações — fail-open). Clippy does *not* exempt
// `#[cfg(test)]` code from that lint, so the spec's "exceto em módulos de
// teste" carve-out is applied explicitly here: under `cfg(test)`, `.unwrap()`
// / `.expect()` are allowed — a panicking assertion *is* a test failure.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `mustard-core` — shared foundation crate for the Mustard Rust migration.
//!
//! This crate concentrates the logic that hooks, scripts, and the CLI all
//! depend on, so the port from JavaScript (epics B3/B4/B5) stays lean instead
//! of re-implementing the same primitives dozens of times.
//!
//! Layers:
//!
//! - [`model`] — pure `serde` data types with zero side effects: the harness
//!   event schema, the hook contract, pipeline-state, and the SDD `ViewModels`
//!   under [`model::view`].
//! - [`fs`] — the single canonical filesystem seam: the [`fs::Fs`] port,
//!   [`fs::real::RealFs`], an in-memory [`fs::memory::FakeFs`], and module-level
//!   free functions that are the drop-in replacement for `std::fs`. Every other
//!   `std::fs` call in the workspace migrates onto this.
//! - [`store`] — side-effecting infrastructure behind traits: the event log,
//!   `pipeline-state` read/write, layered on [`fs`].
//! - [`projection`] — pure folds over `&[HarnessEvent]`: one function per
//!   `ViewModel`. No IO, no side effects — deterministic and testable in
//!   isolation.
//! - [`reader`] — the [`reader::SpecReader`] trait + two adapters:
//!   [`reader::SqliteSpecReader`] (production) and
//!   [`reader::InMemorySpecReader`] (tests). Thin wrappers that supply the
//!   event slice from the store and delegate to `projection`.
//! - [`error`] — the crate's typed error plus fail-open helpers.
//! - cross-cutting foundation — [`config`] (enforcement modes), [`env`] (the
//!   `hook-env.js` port), [`metrics`] (the `metrics-emit.js` port), and
//!   [`knowledge`] (knowledge extraction + the inter-agent context-selection
//!   API).

pub mod config;
pub mod economy;
pub mod env;
pub mod error;
pub mod fs;
pub mod i18n;
pub mod store;
pub mod knowledge;
pub mod meta;
pub mod metrics;
pub mod model;
pub mod process;
pub mod projection;
pub mod reader;
pub mod spec;
pub mod telemetry;

// Root re-exports — consumers can write `use mustard_core::…` without
// remembering which sub-module owns each name.
#[allow(deprecated)] // SpecStatus is re-exported during the W1→W7 migration window.
pub use model::view::SpecStatus;
pub use model::view::{
    AcStatus, AcceptanceCriterion, FileCount, Flags, Outcome, Phase, PhaseSegment, QualityRollup,
    Scope, SegmentState, SpecChild, SpecFilter, SpecState, SpecStatusFilter, SpecSummary,
    SpecTrack, SpecView, Stage, StateError, TimeWindow, TimelineKind, TimelineNode, WaveStatus,
    WaveView, WorkspaceAlert, WorkspaceAlertKind, WorkspaceSummary,
};
pub use reader::{InMemorySpecReader, ReadError, SpecReader, SqliteSpecReader};

// Spec-document I/O — the single canonical owner of parsing / serializing /
// rewriting the lifecycle header of a spec `.md` file. See `spec/mod.rs`.
// Layered on top of the canonical filesystem seam `crate::fs`.
pub use spec::{
    flags_label, header_field, header_region_lines, outcome_label, parse_state, read_state,
    rewrite_header, serialize_header, stage_label, status_word, write_state,
};

// Economy domain re-exports — see `economy/mod.rs` for the full surface.
pub use economy::{EconomyScope, EconomySummary, SavingsSource};

// Meta sidecar — single canonical owner of `meta.json` schema + IO. See
// `meta.rs`. Sidecar replaces the legacy `### Stage:` / `### Outcome:` /
// `### Phase:` / `### Scope:` / `### Lang:` / `### Checkpoint:` / `### Parent:`
// headers under `.claude/spec/**`.
pub use meta::{normalise_lang, read_meta, write_meta, Meta};

// i18n — central language + tone module for Mustard banners. See `i18n.rs`.
// Locale is BCP-47 (`pt-BR`/`en-US`); short forms are rejected with
// `LocaleError::ShortForm` per `project_locale_codes`.
pub use i18n::{apply_tone, slugify, translate, wave_label, I18n, Locale, LocaleError, Tone};
