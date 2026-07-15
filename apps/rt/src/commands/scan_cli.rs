//! The `run` subcommands for the `/scan` chain (mine and enrich the repo model).
//!
//! TWO registrations per command, both in this file: the variant in
//! [`ScanCmd`] AND its arm in [`dispatch`] below. Forgetting the second
//! still compiles, but the command vanishes from the CLI.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run scan <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{scan, scan_equivalences, scan_guards, scan_patterns};

/// The `run` subcommands owned by the `/scan` chain (mine and enrich the repo model).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum ScanCmd {
    /// Mine the workspace into `grain.model.json` via the bundled `scan` tool —
    /// THE scan (replaced the old in-tree miner + per-project skill/agent
    /// generation; the model is the single durable artifact).
    #[command(display_order = 0)]
    Scan {
        /// The workspace root to scan. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output path. Defaults to `<root>/.claude/grain.model.json`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// (Re)generate a lean CLAUDE.md for every subproject found in the
        /// grain model. Only the machine-owned scan-map block is regenerated;
        /// curated sections (Guards, Architecture, …) are preserved verbatim.
        /// Without this flag the command only warns about CLAUDE.md files that
        /// exceed the size threshold.
        #[arg(long)]
        full: bool,
    },

    /// Persist a CONFIRMED vocabulary bridge into the learned-equivalences
    /// overlay (`.claude/grain.equivalences.learned.json`) — the write-back of
    /// a settled `uncovered` row: the existence gate found which code
    /// vocabulary a request concept maps to, and every later query covers it.
    /// The generated `grain.equivalences.json` is never touched, so re-scans
    /// never wipe what was learned. Explicit write only — never automatic.
    #[command(name = "equivalence-learn")]
    #[command(display_order = 2)]
    EquivalenceLearn {
        /// The request-language concept that went uncovered (accent-folded to
        /// the lookup key, e.g. `abas`).
        #[arg(long)]
        term: String,
        /// Comma/space-separated code-vocabulary tokens the concept maps to
        /// (e.g. `tab,tabs`).
        #[arg(long)]
        tokens: String,
        /// Workspace root (holds `.claude/`). Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Enumerate every subproject `CLAUDE.md` whose `## Guards` block is still
    /// `pending` (the Wave-2 enrich hand-off seeded by `scan --full`). Emits a
    /// JSON array `[{path, subproject, kind, frameworks}]` parsed from each
    /// block's facts comment. Excludes the workspace-root unit. Fail-open: any
    /// IO error degrades to `[]` and exit 0.
    #[command(name = "scan-guards-list")]
    #[command(display_order = 61)]
    ScanGuardsList {
        /// Workspace root to walk. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Splice the enrich agent's authored guards into a subproject
    /// `CLAUDE.md`'s pending `## Guards` block: non-destructive (only the span
    /// between the markers changes), line-capped, and idempotent (the marker
    /// flips to its non-pending form so a re-run of `scan-guards-list` skips
    /// it). Refuses the workspace-root `CLAUDE.md`.
    #[command(name = "scan-guards-apply")]
    #[command(display_order = 62)]
    ScanGuardsApply {
        /// Path to the subproject `CLAUDE.md` to enrich.
        #[arg(long)]
        path: PathBuf,
        /// Workspace root the scan ran from. Used to classify whether `path` is
        /// the root unit (refused) or a nested subproject (spliced), via the
        /// same `subproject_of` rule `scan-guards-list` uses. Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Authored guard text, or `-` to read it from stdin. `allow_hyphen_values`
        /// so a body starting with a `-` bullet is not mistaken for a flag.
        #[arg(long, default_value = "-", allow_hyphen_values = true)]
        guards: String,
    },
    /// Derive the missing pattern-skill *mold* worklist from `grain.model.json`:
    /// for each mined role cluster (≥3 members, not under a test/fixture path)
    /// attributed to its subproject, propose a `{subproject}-{role}-pattern` mold
    /// with real hand-written exemplars — skipping any cluster whose mold already
    /// exists, capped at 4 per subproject. Emits a JSON array
    /// `[{subproject, label, slug, moldPath, affix, exemplars, ...}]`. The mold
    /// twin of `scan-guards-list`. Fail-open: a missing/unparseable model → `[]`.
    #[command(name = "scan-patterns-list")]
    #[command(display_order = 63)]
    ScanPatternsList {
        /// Workspace root (must contain `.claude/grain.model.json`). Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Write one enrich-agent-authored pattern mold to its
    /// `{subproject}/.claude/skills/{slug}-pattern/SKILL.md`, CREATE-ONLY (an
    /// existing mold is left untouched) and path-shape-guarded. The mold twin of
    /// `scan-guards-apply`; being a `run` command it sidesteps the
    /// background-isolation gate that blocks the orchestrator's own Write.
    #[command(name = "scan-patterns-apply")]
    #[command(display_order = 64)]
    ScanPatternsApply {
        /// Path to the mold `SKILL.md` to create.
        #[arg(long)]
        path: PathBuf,
        /// Authored SKILL.md body, or `-` to read it from stdin. `allow_hyphen_values`
        /// so a body starting with `-`/`---` frontmatter is not mistaken for a flag.
        #[arg(long, default_value = "-", allow_hyphen_values = true)]
        content: String,
    },
}

/// Dispatch one `scan`-family `run` subcommand.
pub fn dispatch(cmd: ScanCmd) {
    match cmd {
        ScanCmd::Scan { root, out, full } => scan::run(&root, out.as_deref(), full),
        ScanCmd::EquivalenceLearn { term, tokens, root } => scan_equivalences::run_learn(&root, &term, &tokens),
        ScanCmd::ScanGuardsList { root } => scan_guards::list::run(&root),
        ScanCmd::ScanGuardsApply { path, root, guards } => {
            scan_guards::apply::run(&path, &root, &guards)
        }
        ScanCmd::ScanPatternsList { root } => scan_patterns::list::run(&root),
        ScanCmd::ScanPatternsApply { path, content } => {
            scan_patterns::apply::run(&path, &content)
        }
    }
}
