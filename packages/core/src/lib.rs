#![forbid(unsafe_code)]
// `clippy::unwrap_used` is `deny` workspace-wide so no hook-path code can
// panic (fail-open). Clippy does *not* exempt
// `#[cfg(test)]` code from that lint, so the spec's "exceto em módulos de
// teste" carve-out is applied explicitly here: under `cfg(test)`, `.unwrap()`
// / `.expect()` are allowed — a panicking assertion *is* a test failure.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! `mustard-core` — shared foundation crate for the Mustard Rust migration.
//!
//! This crate concentrates the logic that hooks, scripts, and the CLI all
//! depend on, so the port from JavaScript stays lean instead
//! of re-implementing the same primitives dozens of times.
//!
//! Layers:
//!
//! - [`model`] — pure `serde` data types with zero side effects: the harness
//!   event schema, the hook contract, pipeline-state, and the SDD `ViewModels`
//!   under [`model::view`].
//! - [`fs`] — the single canonical filesystem seam: the [`fs::Fs`] port,
//!   [`fs::real::RealFs`], and module-level
//!   free functions that are the drop-in replacement for `std::fs`. Every other
//!   `std::fs` call in the workspace migrates onto this.
//! - [`events`] — NDJSON event primitives ([`Event`] / [`EventReader`]) plus
//!   the per-spec workspace walker; the canonical event store for the
//!   no-sqlite migration. Layered on [`fs`].
//! - [`projection`] — pure folds over `&[HarnessEvent]`: one function per
//!   `ViewModel`. No IO, no side effects — deterministic and testable in
//!   isolation. Production callers in `apps/rt` and `apps/dashboard` feed the
//!   slice from [`projection::read_workspace_events`] (NDJSON walker).
//! - [`error`] — the crate's typed error plus fail-open helpers.
//! - cross-cutting foundation — [`config`] (enforcement modes), [`env`] (the
//!   `hook-env.js` port), and [`metrics`] (the `metrics-emit.js` port).

// Root re-exports — consumers can write `use mustard_core::…` without
// remembering which sub-module owns each name.
pub mod io;
pub mod domain;
pub mod view;
pub use platform::time;
pub mod platform;
// Harness hook-command resolution — rewrites a copied `.claude/settings.json` so
// every hook invokes `mustard-rt` by absolute path (dropping the redundant `rtk`
// prefix), making the harness PATH-independent. Shared by `mustard` init/update
// and `mustard-rt run rehook`. See `platform/hook_resolve.rs`.
pub use platform::hook_resolve::{
    resolve_mustard_rt, rewrite_command, rewrite_hooks_value, rewrite_settings_hooks,
};
// Project seeding — the compiled-in seed payload (`seeds`) and the
// install/update engine (`project_seed`) shared by `mustard init` and
// `mustard-rt run upsert`. See `platform/seeds.rs` + `platform/project_seed.rs`.
pub use platform::project_seed::{
    carries_private_marks, default_inject_entries, detect_install_mode, footprint,
    footprint_pathspecs, footprint_rules, injectable_declared_paths, injectable_names,
    injectable_seeds, is_written_footprint, migrate_orchestrator_footprint,
    record_version_stamp, record_written_path, retire_planted_plugin_enablement, seed_gitignore,
    seed_injectable_files, seed_settings, upsert_project, worktree_is_clean, FootprintEntry,
    InstallMode, MigrationOutcome, RecordOutcome, SeedOutcome, UpsertReport, CLAUDE_LOCAL_MD,
    CLAUDE_MD,
    PRIVATE_MARKS,
};
// Clone-local git exclude — the layer a private install hides its footprint in,
// resolved through `git rev-parse --git-path info/exclude` (never the literal
// `.git/info/exclude`, which is absent in a submodule or a linked worktree).
// See `platform/git_exclude.rs`.
pub use platform::git_branches::{
    branch_catalog, current_branch, default_branch, protected_branches, remote_branch_names,
    BranchEntry,
};
pub use platform::git_provider::{detect_provider, provider_of_url, resolve_provider};
pub use platform::git_exclude::{
    ensure_excluded, exclude_file, tracked_paths, ExcludeFailure, ExcludeOutcome,
};
// The last three are the plugin registry itself — its path, the config dir it
// sits in, and the key half this harness ships under. Public so `apps/rt` reads
// the SAME registry this crate does: a second copy drifts the day the host moves
// the file, and the caller degrades to a permanent silent skip no test can see.
pub use platform::harness::{
    claude_config_dir, harness_version, installed_harness_version, installed_harness_version_from,
    installed_plugin_rt, installed_plugin_rt_from, is_behind, newer_installed_rt,
    newer_installed_rt_from, INSTALLED_PLUGINS, PLUGIN_NAME,
};
pub use platform::seeds::{
    CLAUDE_GITIGNORE, DISPATCH_MD, MATERIAL_MD, ORCHESTRATOR_MD, SETTINGS_SEED,
};

pub use domain::model::view::{
    Flags, Outcome, Phase, Scope, SpecChild, SpecState, SpecSummary, SpecView, Stage, StateError,
};
// Spec-document I/O — the single canonical owner of parsing / serializing /
// rewriting the lifecycle header of a spec `.md` file. See `spec/mod.rs`.
// Layered on top of the canonical filesystem seam `crate::io::fs`.
pub use domain::spec::{
    flags_label, header_field, header_region_lines, outcome_label, parse_state, read_state,
    rewrite_header, serialize_header, stage_label, status_word, write_state,
};

// Project config — the single source of truth for `<root>/mustard.json`
// (schema + IO + accessors). Replaces the scattered ad-hoc parsers
// (`mustard_config`, `git_flow::MustardConfig`, `read_mustard_tone`, …). See
// `domain/config.rs`.
pub use domain::config::{
    glob_matches, Amend, Commands, GateModes, GitConfig, Injectable, Language, LanguageConfig,
    ProjectConfig, RolePattern, Runtime, Subprojects, BUILD_COMMAND_FALLBACK,
};
// Agnostic build/test/lint/type-check command detection (`detect_commands` for
// `init`, `detect_commands_for_unit` for the per-subproject `scan` pass). See
// `domain/command_detect.rs`.
pub use domain::command_detect::{detect_commands, detect_commands_for_unit};

// scan tool client — the single boundary to the external `scan` miner (scan /
// digest / facts / spec / verify). Replaces the deleted in-tree scan engine;
// Mustard consumes the tool's JSON/Markdown, never project source — and never
// parses `grain.model.json` itself (the scan tool owns that schema). See
// `domain/scan.rs`.
pub use domain::scan::{read_entity_names, read_projects, DigestQuery, ModelFacts, Project, Scan};

// Source-language resolution — the single owner of "what language is this target
// (a set of file paths), and can the JS/TS-family gates reason about it?".
// Consulted by `dependency-precheck` and `wave-size-check` so both loosen
// consistently on a non-JS/TS subproject. See `domain/source_lang.rs`.
pub use domain::source_lang::{resolve_target_languages, target_understood};

// i18n — central language module for Mustard banners. See `i18n.rs`.
//
// Two locale types live here, doing two different jobs:
// - `SupportedLocale` — the closed catalogue Mustard ships translations for
//   (`pt-BR` / `en-US`). Drives `translate` / `I18n`. Short forms (`pt` /
//   `en`) are rejected with `LocaleError::ShortForm` per
//   `project_locale_codes`.
// - `UserLocale` — the open locale a spec records. Accepts any BCP-47-shaped
//   code (`fr-FR`, `de-DE`, `en-GB`, ...). Parsed into a `SupportedLocale`
//   when a banner needs to render.
//
// The project's own language is not read here: `ProjectConfig::language` is
// its one reader.
pub use platform::i18n::{
    slugify, translate, wave_label, I18n, LocaleError, SupportedLocale, UserLocale,
    UserLocaleError,
};

// Canonical `.claude/` path catalog — every consumer in `apps/rt` builds a
// `ClaudePaths` once and then asks for a typed accessor instead of joining
// strings inline. See `claude_paths.rs`.
pub use io::claude_paths::{ClaudePaths, ClaudePathsError, SpecPaths, WavePaths};

// Canonical workspace-root resolver — single source of truth for "the
// directory that contains `mustard.json` + `.claude/`". See `workspace.rs`.
pub use io::workspace::{workspace_root, WorkspaceError};

// Summary document — the versionable `.summary.json` artefact committed to
// git alongside each spec. Re-exported at root so consumers can write
// `mustard_core::SpecSummaryDoc` without knowing the sub-module path.
pub use view::summary::SpecSummaryDoc;

// Vocabulary matcher — the four-layer term scanner used by the regression
// gate. Layers are EN identifiers per the hard rule
// (`Semantic`, `Pattern`, `Keyword`, `Noise`); the on-disk TOML keys are
// lowercased copies of the same names.
pub use domain::vocabulary::{
    check_layer_promotion, Layer, PromotionVerdict, ScanHit, VocabError, VocabLayer,
    VocabularyDoc, VocabularyMatcher,
};
