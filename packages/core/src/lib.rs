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
//! - [`io`] — the ports to the outside world: the filesystem seam
//!   ([`io::fs`]), the NDJSON spec event store, the project map database and
//!   the other readers and writers of files.
//! - [`domain`] — the pure rules and data types: the hook contract
//!   ([`domain::model`]), the spec event log, config, search and the wave
//!   request.
//! - [`platform`] — the pieces tied to the machine and the harness: git, the
//!   installed plugin, the project seed, the text catalogue ([`platform::i18n`])
//!   and the typed error.
//! - [`view`] — the spec page document.

// Root re-exports — consumers can write `use mustard_core::…` without
// remembering which sub-module owns each name.
pub mod io;
pub mod domain;
pub mod view;
pub use platform::time;
pub mod platform;

// A pasta do pacote lida na hora de rodar, para os testes de dentro de `src/`.
#[cfg(test)]
#[path = "../tests/support/manifest_dir.rs"]
pub(crate) mod manifest_dir;
// Project seeding — the compiled-in seed payload (`seeds`) and the
// install/update engine (`project_seed`) shared by `mustard init` and
// `mustard-rt run upsert`. See `platform/seeds.rs` + `platform/project_seed/`.
pub use platform::project_seed::{
    carries_private_marks, default_inject_entries, detect_install_mode, footprint,
    footprint_pathspecs, footprint_rules, harness_text_paths, harness_texts, is_written_footprint,
    migrate_inject_declarations, output_style_for, retire_planted_plugin_enablement,
    seed_gitignore, seed_harness_texts, seed_settings, session_map_declared_path,
    upsert_project, CleanupDone, CleanupPlan, FootprintEntry, InstallMode, SeedOutcome, Switches,
    UpsertReport, CLAUDE_LOCAL_MD, CLAUDE_MD, PRIVATE_MARKS, RTK_HOOK_COMMAND,
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
    claude_config_dir, development_rt, development_rt_in, harness_version, installed_harness_version, installed_harness_version_from,
    installed_plugin_rt, installed_plugin_rt_from, is_behind, newer_installed_rt,
    newer_installed_rt_from, INSTALLED_PLUGINS, PLUGIN_NAME,
};
pub use platform::seeds::{
    agent_texts, session_map, AGENT_NAMES, CLAUDE_GITIGNORE, SESSION_MAP_NAME, SETTINGS_SEED,
};

// Project config — the single source of truth for `<root>/mustard.json`
// (schema + IO + accessors). Replaces the scattered ad-hoc parsers (the
// runtime accessors, the CLI writer, one reader per feature). See
// `domain/config.rs`.
pub use domain::config::{
    glob_matches, Amend, Commands, GitConfig, Injectable, Language, LanguageConfig,
    FilterSetting, MapConfig, ProjectConfig, Runtime, SearchConfig, Setting, Subprojects, BUILD_COMMAND_FALLBACK,
};
// Agnostic build/test/lint/type-check command detection (`detect_commands` for
// `init`, `detect_commands_for_unit` for the per-subproject `scan` pass). See
// `domain/command_detect.rs`.
pub use domain::command_detect::{detect_commands, detect_commands_for_unit};

// scan tool client — the single boundary to the external `scan` miner (the
// `scan` pass). Replaces the deleted in-tree scan engine;
// Mustard consumes the tool's JSON/Markdown, never project source — and never
// reads the map `.claude/grain.db` outside the port `io/project_map.rs` (the
// scan tool fills its blocks). See `domain/scan.rs`.
pub use domain::scan::{read_projects, Project, Scan};

// i18n — central language module for Mustard banners. See `i18n.rs`.
//
// `SupportedLocale` is the closed catalogue Mustard ships translations for
// (`pt-BR` / `en-US`). It drives `translate` / `I18n`. Short forms (`pt` /
// `en`) are rejected with `LocaleError::ShortForm` per `project_locale_codes`.
//
// The project's own language is not read here: `ProjectConfig::language` is
// its one reader.
pub use platform::i18n::{translate, wave_label, I18n, LocaleError, SupportedLocale};

// Canonical `.claude/` path catalog — every consumer in `apps/rt` builds a
// `ClaudePaths` once and then asks for a typed accessor instead of joining
// strings inline. See `claude_paths.rs`.
pub use io::claude_paths::{ClaudePaths, ClaudePathsError, SpecPaths};

// Canonical workspace-root resolver — single source of truth for "the
// directory that contains `mustard.json` + `.claude/`". See `workspace.rs`.
pub use io::workspace::{mustard_checkout, workspace_root, WorkspaceError};

// Vocabulary errors — the typed error of the stack registry. See
// `domain/vocabulary/mod.rs`.
pub use domain::vocabulary::VocabError;
