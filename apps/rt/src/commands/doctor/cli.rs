//! The `run` subcommands for health checks and audits (`doctor/`).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`DoctorCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run doctor <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;

use crate::commands::{doctor};

/// The `run` subcommands owned by health checks and audits (`doctor/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum DoctorCmd {
    /// Read-only installation health diagnostic: wiring, drift, state health,
    /// wave-integrity, claude-paths, workspace-leaks, i1, and (optionally)
    /// residue. Prints a compact OK/WARN/FAIL report and exits 1 if any
    /// category is FAIL, 0 otherwise.
    ///
    /// Pass `--json` as a shortcut for `--format json` (W10.T10.6).
    #[command(display_order = 41)]
    Doctor {
        /// Also scan for dead file/script references (slower).
        #[arg(long)]
        residue: bool,
        /// Run a specific named check in isolation: `skill-discovery`,
        /// `wave-integrity`, `claude-paths` (W3.T3.4), `workspace-leaks`
        /// (W3.T3.8), `i1` (W3.T3.9), or `superseded` (prune candidates —
        /// terminal / stale-anchored specs the maintainer can archive).
        #[arg(long)]
        check: Option<String>,
        /// Output format: `text` (default) or `json`.
        #[arg(long, default_value = "text")]
        format: String,
        /// Shorthand for `--format json` (W10.T10.6).
        #[arg(long)]
        json: bool,
    },
    /// Scan markdown docs for obsolete terms declared in `.claude/.docs-audit.json`.
    ///
    /// Emits a JSON report of stale-doc hits. With `--strict` (or env
    /// `MUSTARD_DOCS_AUDIT_MODE=strict` set by the caller), exits `1` when any
    /// hit is found — the close gate uses this to block CLOSE on narrative
    /// drift after an architectural spec lands.
    #[command(display_order = 46)]
    DocsStaleCheck {
        /// Limit the audit to a single spec (`from_spec` field). Defaults to
        /// running every audit declared in the registry.
        #[arg(long)]
        from: Option<String>,
        /// Exit `1` when any hit is found. Default is warn-only (exit `0`).
        #[arg(long)]
        strict: bool,
        /// Also recurse into nested `apps/*/.claude/**` installed-payload copies.
        /// Default `false` — the audit scans only source-of-truth docs (the
        /// repo-root `.claude/` tree and each subproject's root `CLAUDE.md`).
        /// Equivalent to `MUSTARD_DOCS_AUDIT_INCLUDE_NESTED=1`.
        #[arg(long)]
        include_nested: bool,
    },
    /// Audit source files for pt-BR prose in EN-only files (diacritic-seed
    /// heuristic). Warn-only by default; `--strict` exits `1` on any hit.
    #[command(display_order = 47)]
    LanguageAudit {
        /// Output format: `text` (default) or `json`.
        #[arg(long, default_value = "text")]
        format: String,
        /// Exit `1` when any hit is found. Default is warn-only (exit `0`).
        #[arg(long)]
        strict: bool,
    },
}

/// Dispatch one `doctor`-family `run` subcommand.
pub fn dispatch(cmd: DoctorCmd) {
    match cmd {
        DoctorCmd::Doctor { residue, check, format, json } => {
            // `--json` is a shorthand for `--format json` (W10.T10.6).
            let effective_format = if json { "json".to_string() } else { format };
            doctor::doctor::run(doctor::doctor::DoctorOpts {
                residue,
                check,
                format: effective_format,
            });
        }
        DoctorCmd::DocsStaleCheck { from, strict, include_nested } => {
            doctor::docs_stale_check::run(from.as_deref(), strict, include_nested);
        }
        DoctorCmd::LanguageAudit { format, strict } => {
            doctor::language_audit::run(doctor::language_audit::LanguageAuditOpts { format, strict });
        }
    }
}
