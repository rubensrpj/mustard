//! The `run` subcommands for installation maintenance (`maint/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`MaintCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run maint <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{maint};

/// The `run` subcommands owned by installation maintenance (`maint/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum MaintCmd {
    /// Check (or apply) freshness of managed artifacts against their upstreams.
    ///
    /// Maintainer-side: reads `apps/cli/templates/.artifacts.json` and probes
    /// each external upstream. Fail-open — network errors degrade an artifact
    /// to `unknown` and never fail the command.
    #[command(display_order = 50)]
    ArtifactUpdate {
        /// Probe upstreams and emit the JSON freshness report (the default).
        #[arg(long)]
        check: bool,
        /// Pull updates into vendored trees / bump pinned versions.
        #[arg(long)]
        apply: bool,
        /// Manifest path (default `apps/cli/templates/.artifacts.json`).
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Garbage-collect orphan Claude agent worktrees under
    /// `<repo>/.claude/worktrees/agent-*`.
    ///
    /// Enumerates the directory, computes each entry's age (via
    /// `<repo>/.git/worktrees/<name>/HEAD` mtime, falling back to the dir's
    /// own mtime), and reports/removes entries older than `--age-days N`
    /// (default 7). Dry-run by default; `--apply` is required to mutate the
    /// filesystem. Emits `pipeline.economy.operation.invoked` to the harness
    /// event store.
    #[command(display_order = 56)]
    WorktreeGc {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Age threshold in whole days. Worktrees older than this are
        /// eligible for removal.
        #[arg(long = "age-days", default_value_t = maint::worktree_gc::DEFAULT_AGE_DAYS)]
        age_days: u32,
        /// Preview only — no filesystem mutation (the default).
        #[arg(long, default_value_t = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Apply the removal. Required to mutate the filesystem.
        #[arg(long)]
        apply: bool,
    },
    /// Kill-switch: rename `.claude/settings.json` → `.disabled-<ts>` and wipe
    /// volatile harness state (`.agent-state/`, `.cluster-cache.json`,
    /// `.worktrees/`). Restore with [`Self::Rehook`].
    ///
    /// `--scope this` (default) acts on the current repo's `.claude/` only.
    /// `--scope monorepo` also sweeps every `apps/*/.claude/` +
    /// `packages/*/.claude/`. `--scope all` adds the user-global
    /// `~/.claude/settings.json`, gated by `--confirm` (otherwise reported as
    /// `state: "skipped"`). Emits a pretty JSON report.
    #[command(display_order = 57)]
    Unhook {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Scope: `this` (default), `monorepo`, or `all`.
        #[arg(long, default_value = "this")]
        scope: String,
        /// Required for `--scope all` to also touch the user-global
        /// `~/.claude/settings.json`.
        #[arg(long)]
        confirm: bool,
    },
    /// Reverse [`Self::Unhook`]: in each `.claude/` in scope, rename the
    /// newest `settings.json.disabled*` snapshot back to `settings.json`.
    /// Volatile state directories that `unhook` wiped are left alone — the
    /// runtime regenerates them on the next run. Emits a pretty JSON report.
    #[command(display_order = 58)]
    Rehook {
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long, default_value = "this")]
        scope: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Audit (and optionally remove) drift in a project's `.claude/` directory.
    ///
    /// Enumerates every direct child of `.claude/`, classifies each against a
    /// declared consumer list (KEEP / STALE / ORPHAN / LEGACY / CACHE), and
    /// either reports candidates (default `--dry-run`) or removes the ORPHAN
    /// / LEGACY ones (`--apply`). Emits byte-stable pretty JSON; fail-open at
    /// every step — exit code is always 0.
    #[command(name = "claude-dir-prune")]
    #[command(display_order = 66)]
    ClaudeDirPrune {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Preview only — emit the report, mutate nothing (the default).
        #[arg(long, default_value_t = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Apply the removals. Required to mutate the filesystem.
        #[arg(long)]
        apply: bool,
        /// Reserved for parity with sibling subcommands — JSON is the only
        /// format today, but the flag exists so callers can pass it.
        #[arg(long)]
        json: bool,
    },
    /// W5.T5.6 — Generate `.cursorrules` from the repo's `CLAUDE.md` tree.
    #[command(name = "adapt-cursor")]
    #[command(display_order = 72)]
    AdaptCursor {
        /// Repo root override.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Preview only — no filesystem mutation.
        #[arg(long)]
        dry_run: bool,
    },
    /// Refresh stale `.claude/` installs after edits in `apps/cli/templates/`.
    ///
    /// Walks `apps/cli/templates/{refs,commands/mustard,skills}/**`, SHA-256
    /// compares each source against the consumer `.claude/<sub>/`, and copies
    /// divergent files. Generated artefacts (`grain.model.json`, caches)
    /// and volatile state dirs are excluded. Emits `{copied, skipped,
    /// conflicts, errors}` JSON. Fail-open; exit code is always 0.
    #[command(name = "refresh-claude")]
    #[command(display_order = 73)]
    RefreshClaude {
        /// Target consumer directory (the project whose `.claude/` to refresh).
        /// Defaults to the current working directory.
        #[arg(long)]
        target: Option<PathBuf>,
        /// Preview only — compare and report, but do NOT write any files.
        #[arg(long)]
        dry_run: bool,
        /// Override the templates source directory (defaults to auto-discovery).
        #[arg(long = "templates-dir")]
        templates_dir: Option<PathBuf>,
    },
    /// W5.T5.7a — Install dependencies in every detected subproject.
    #[command(name = "maint-deps")]
    #[command(display_order = 74)]
    MaintDeps {
        /// Preview only — print the resolved install commands without running.
        #[arg(long)]
        dry_run: bool,
    },
    /// W5.T5.7b — Run build/type-check validation in every detected subproject.
    #[command(name = "maint-validate")]
    #[command(display_order = 75)]
    MaintValidate {
        /// Preview only — print the resolved validate commands without running.
        #[arg(long)]
        dry_run: bool,
    },
}

/// Dispatch one `maint`-family `run` subcommand.
pub fn dispatch(cmd: MaintCmd) {
    match cmd {
        MaintCmd::ArtifactUpdate {
            check,
            apply,
            manifest,
        } => maint::artifact_update::run(check, apply, manifest.as_deref()),
        MaintCmd::WorktreeGc {
            repo,
            age_days,
            dry_run,
            apply,
        } => {
            // `dry_run` defaults to `true`; clap's `conflicts_with` blocks
            // passing both. `--apply` is the authoritative mutator flag.
            let _ = dry_run;
            maint::worktree_gc::run(maint::worktree_gc::WorktreeGcOpts {
                repo,
                age_days,
                apply,
            });
        }
        MaintCmd::Unhook { repo, scope, confirm } => {
            maint::unhook::run(maint::unhook::UnhookOpts { repo, scope, confirm });
        }
        MaintCmd::Rehook { repo, scope, confirm } => {
            maint::rehook::run(maint::rehook::RehookOpts { repo, scope, confirm });
        }
        MaintCmd::ClaudeDirPrune {
            repo,
            dry_run,
            apply,
            json,
        } => {
            // `dry_run` defaults to `true`; clap's `conflicts_with` blocks
            // both flags from coexisting. `--apply` is the authoritative
            // mutator flag.
            let _ = dry_run;
            maint::claude_dir_prune::run(maint::claude_dir_prune::ClaudeDirPruneOpts {
                repo,
                apply,
                json,
            });
        }
        MaintCmd::AdaptCursor { repo, dry_run } => {
            maint::adapt_cursor::run(maint::adapt_cursor::AdaptCursorOpts { repo, dry_run });
        }
        MaintCmd::RefreshClaude { target, dry_run, templates_dir } => {
            maint::refresh_claude::run(maint::refresh_claude::RefreshClaudeOpts {
                target,
                dry_run,
                templates_dir,
            });
        }
        MaintCmd::MaintDeps { dry_run } => {
            maint::maint_deps::run(maint::maint_deps::MaintDepsOpts { dry_run });
        }
        MaintCmd::MaintValidate { dry_run } => {
            maint::maint_validate::run(maint::maint_validate::MaintValidateOpts { dry_run });
        }
    }
}
