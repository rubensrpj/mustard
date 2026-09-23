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

use crate::commands::scan;

/// The `run` subcommands owned by the `/scan` chain (mine and enrich the repo model).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum ScanCmd {
    /// Mine the workspace into `grain.model.json` via the bundled `scan` tool —
    /// THE scan (replaced the old in-tree miner + per-project skill/agent
    /// generation; the model is the single durable artifact).
    #[command(display_order = 15)]
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
    /// `tests --file`, `slice --file --name <declaration>` (the declaration's
    /// own lines, without opening the file), `users --name <declaration>` (who
    /// uses it, as `file:line:caller`; `--file` keeps the one declared in that
    /// file), `search --query`, `summary` (the session-start digest, up to
    /// 3 kB) or `skill --path <SKILL.md>` (every cited path exists and the
    /// skill stays under 500 lines). Reads
    /// `.claude/grain.model.json`; prints JSON and exits 1 on a refusal.
    #[command(display_order = 16)]
    Map {
        /// The question to ask.
        #[arg(value_enum)]
        question: crate::commands::map::Question,
        /// The file the question is about (for `examples`, the file the task
        /// creates or changes, or its folder; for `users`, optional, keeps the
        /// declaration of that file).
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
        /// The declaration the question is about (`slice`, `users`).
        #[arg(long)]
        name: Option<String>,
        /// Any directory inside the project. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },

}

/// Dispatch one `scan`-family `run` subcommand.
pub fn dispatch(cmd: ScanCmd) {
    match cmd {
        ScanCmd::Scan { root, out, full } => scan::run(&root, out.as_deref(), full),
        ScanCmd::Map { question, file, task, query, path, name, root } => {
            crate::commands::map::run(&crate::commands::map::MapOpts {
                root,
                question,
                file,
                task,
                query,
                path,
                name,
            });
        }
    }
}
