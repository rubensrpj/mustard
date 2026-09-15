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

use crate::commands::{scan, scan_equivalences};

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
        /// (Re)generate the mustard-owned `.claude/scan-map.md` for every
        /// subproject found in the grain model. No `CLAUDE.md` is ever
        /// written. Without this flag only the model is written.
        #[arg(long)]
        full: bool,
    },

    /// Ask the project map a short question: `examples` for a task
    /// (`--file <target>` or `--task "<task>"`), `importers --file`,
    /// `tests --file`, `search --query`, `summary` (the session-start digest,
    /// up to 3 kB) or `skill --path <SKILL.md>` (every cited path exists and
    /// the skill stays under 500 lines). Reads `.claude/grain.model.json`;
    /// prints JSON and exits 1 on a refusal.
    #[command(display_order = 51)]
    Map {
        /// The question to ask.
        #[arg(value_enum)]
        question: crate::commands::map::Question,
        /// The file the question is about (for `examples`, the file the task
        /// creates or changes, or its folder).
        #[arg(long)]
        file: Option<String>,
        /// The task, in words, when there is no target file (`examples`).
        #[arg(long)]
        task: Option<String>,
        /// The words to look for (`search`).
        #[arg(long)]
        query: Option<String>,
        /// The skill to check (`skill`).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Any directory inside the project. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
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
    /// Print the LAPIDATION KIT: how this project names things — the mined
    /// roles (what a thing is called and where that kind lives), the shapes
    /// (roles that recur together, i.e. what a new entity usually needs) and
    /// the units. Read it BEFORE `run feature` and map the request onto these
    /// words; a request in the code's own vocabulary is the difference between
    /// a withheld answer and the implementing modules. It never reads the
    /// prompt and never suggests — the menu is deterministic, the choice is
    /// yours. Fail-open: a missing/unparseable model prints the empty kit.
    #[command(name = "scan-lapidation")]
    #[command(display_order = 68)] // appended at the tail: slots are a global gapless permutation (see tests/run_command_surface.rs)
    ScanLapidation {
        /// Workspace root (must contain `.claude/grain.model.json`). Defaults to `.`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `scan`-family `run` subcommand.
pub fn dispatch(cmd: ScanCmd) {
    match cmd {
        ScanCmd::Scan { root, out, full } => scan::run(&root, out.as_deref(), full),
        ScanCmd::Map { question, file, task, query, path, root } => {
            crate::commands::map::run(&crate::commands::map::MapOpts { root, question, file, task, query, path });
        }
        ScanCmd::EquivalenceLearn { term, tokens, root } => scan_equivalences::run_learn(&root, &term, &tokens),
        ScanCmd::ScanLapidation { root } => crate::commands::lapidation::run(&root),
    }
}
