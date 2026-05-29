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
//! - [`events`] — NDJSON event primitives ([`Event`] / [`EventReader`]) plus
//!   the per-spec workspace walker; the canonical event store for the
//!   no-sqlite migration (W2-W8). Layered on [`fs`].
//! - [`projection`] — pure folds over `&[HarnessEvent]`: one function per
//!   `ViewModel`. No IO, no side effects — deterministic and testable in
//!   isolation. Production callers in `apps/rt` and `apps/dashboard` feed the
//!   slice from [`projection::read_workspace_events`] (NDJSON walker).
//! - [`error`] — the crate's typed error plus fail-open helpers.
//! - cross-cutting foundation — [`config`] (enforcement modes), [`env`] (the
//!   `hook-env.js` port), [`metrics`] (the `metrics-emit.js` port), and
//!   [`knowledge`] (knowledge extraction + the inter-agent context-selection
//!   API).

// Snapshot-and-compare primitive consumed by the Wave 4 regression gate.
// Reuses `ast::GrammarLoader` / `ast::TreeSitterParser` for the precise path
// and falls back to a textual diff (via `similar = "2"`) when no grammar is
// installed for the file's language.

// Root re-exports — consumers can write `use mustard_core::…` without
// remembering which sub-module owns each name.
pub mod io;
pub mod domain;
pub mod view;
pub use platform::time;
pub mod platform;

#[allow(deprecated)] // SpecStatus is re-exported during the W1→W7 migration window.
pub use domain::model::view::SpecStatus;
pub use domain::model::view::{
    AcStatus, AcceptanceCriterion, FileCount, Flags, Outcome, Phase, PhaseSegment, QualityRollup,
    Scope, SegmentState, SpecChild, SpecFilter, SpecState, SpecStatusFilter, SpecSummary,
    SpecTrack, SpecView, Stage, StateError, TimeWindow, TimelineKind, TimelineNode, WaveStatus,
    WaveView, WorkspaceAlert, WorkspaceAlertKind, WorkspaceSummary,
};
// Spec-document I/O — the single canonical owner of parsing / serializing /
// rewriting the lifecycle header of a spec `.md` file. See `spec/mod.rs`.
// Layered on top of the canonical filesystem seam `crate::io::fs`.
pub use domain::spec::{
    flags_label, header_field, header_region_lines, outcome_label, parse_state, read_state,
    rewrite_header, serialize_header, stage_label, status_word, write_state,
};

// Economy domain re-exports — see `economy/mod.rs` for the full surface.
pub use domain::economy::{EconomyScope, EconomySummary, SavingsSource};

// Project config — the single source of truth for `<root>/mustard.json`
// (schema + IO + accessors). Replaces the scattered ad-hoc parsers
// (`mustard_config`, `git_flow::MustardConfig`, `read_mustard_tone`, …). See
// `domain/config.rs`.
pub use domain::config::{
    glob_matches, Amend, Commands, GateModes, GitConfig, ProjectConfig, RolePattern, Runtime,
    Subprojects, BUILD_COMMAND_FALLBACK,
};
// Agnostic build/test/lint/type-check command detection (used by `init` and
// `scan`). See `domain/command_detect.rs`.
pub use domain::command_detect::detect_commands;

// Meta sidecar — single canonical owner of `meta.json` schema + IO. See
// `meta.rs`. Sidecar replaces the legacy `### Stage:` / `### Outcome:` /
// `### Phase:` / `### Scope:` / `### Lang:` / `### Checkpoint:` / `### Parent:`
// headers under `.claude/spec/**`.
pub use domain::meta::{normalise_lang, read_meta, write_meta, Meta, MetaFlags};

// i18n — central language + tone module for Mustard banners. See `i18n.rs`.
//
// Two locale types live here, doing two different jobs:
// - `SupportedLocale` — the closed catalogue Mustard ships translations for
//   (`pt-BR` / `en-US`). Drives `translate` / `apply_tone` / `I18n`. Short
//   forms (`pt` / `en`) are rejected with `LocaleError::ShortForm` per
//   `project_locale_codes`.
// - `UserLocale` — the open user-declared locale parsed out of
//   `mustard.json#specLang` and `### Lang:` headers. Accepts any
//   BCP-47-shaped code (`fr-FR`, `de-DE`, `en-GB`, ...). Bridges to
//   `SupportedLocale` via `user.to_supported().unwrap_or_default()` when a
//   banner needs to render.
//
// W7 — every callsite now uses `SupportedLocale` (catalogue) or `UserLocale`
// (user-declared). The deprecated `Locale` alias was removed.
pub use platform::i18n::{
    apply_tone, slugify, translate, wave_label, I18n, LocaleError, SupportedLocale, Tone,
    UserLocale, UserLocaleError,
};

// Canonical `.claude/` path catalog — every consumer in `apps/rt` builds a
// `ClaudePaths` once and then asks for a typed accessor instead of joining
// strings inline. See `claude_paths.rs`.
pub use io::claude_paths::{ClaudePaths, ClaudePathsError, SpecPaths, WavePaths};

// Canonical workspace-root resolver — single source of truth for "the
// directory that contains `mustard.json` + `.claude/`". See `workspace.rs`.
pub use io::workspace::{workspace_root, WorkspaceError};

// Atomic markdown layer — shared by memory/knowledge/spec readers and the
// wikilink footer hook. See `atomic_md/mod.rs`.
pub use io::atomic_md::{MarkdownDoc, MarkdownStore};

// Summary document — the versionable `.summary.json` artefact committed to
// git alongside each spec. Re-exported at root so consumers can write
// `mustard_core::SpecSummaryDoc` without knowing the sub-module path.
pub use view::summary::SpecSummaryDoc;

// NDJSON event primitives — shared by all no-sqlite sub-specs (W2-W7).
// `Event` is the single row unit; `EventReader` provides streaming, cached,
// and filtered access without loading full files into memory.
pub use io::events::{Event, EventReader};

// Vocabulary matcher — the four-layer term scanner used by the regression
// gate (Spec A / Wave 1). Layers are EN identifiers per the wave-0 hard rule
// (`Semantic`, `Pattern`, `Keyword`, `Noise`); the on-disk TOML keys are
// lowercased copies of the same names.
pub use domain::vocabulary::{
    check_layer_promotion, Layer, PromotionVerdict, ScanHit, VocabError, VocabLayer,
    VocabularyDoc, VocabularyMatcher,
};
