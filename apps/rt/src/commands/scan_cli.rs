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
    /// Mine the workspace into the SQLite map `grain.db` with the bundled `scan`
    /// tool; only the blocks that changed are written again.
    /// This is the one scan of the project, and the model is the single
    /// durable artifact. It replaced the old in-tree miner and the per-project
    /// skill and agent generation.
    #[command(display_order = 15)]
    Scan {
        /// The workspace root to scan. Defaults to the current directory.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output path. Defaults to `<root>/.claude/grain.db`.
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
    /// file), `history --name <declaration>` (the commits of the base branch
    /// that changed it, newest first, with title and pull request number;
    /// `--file` picks the file when the name lives in more than one), `search
    /// --query "<words>" --intent "<sentence>"` (the words and the likely
    /// names in `--query`, the sentence of what you look for and why in
    /// `--intent`; part of a name only in `--query`), `summary` (the
    /// session-start summary, up to
    /// 3 kB), `skill --path <SKILL.md>` (every cited path exists and the
    /// skill stays under 500 lines) or `dump` (the map database table by
    /// table, in a fixed order, for debugging). Reads `.claude/grain.db`;
    /// prints JSON and exits 1 on a refusal.
    #[command(display_order = 16)]
    Map {
        /// The question to ask.
        #[arg(value_enum)]
        question: crate::commands::map::Question,
        /// The file the question is about (for `examples`, the file the task
        /// creates or changes, or its folder; for `users` and `history`,
        /// optional, keeps the declaration of that file).
        #[arg(long)]
        file: Option<String>,
        /// The task, in words, when there is no target file (`examples`).
        #[arg(long)]
        task: Option<String>,
        /// The words to look for (`search`): the words of the request and the
        /// likely names in the code. Part of a name goes only here.
        #[arg(long)]
        query: Option<String>,
        /// The sentence of what you are looking for and why (`search`); the
        /// filter reads it, and the search without a filter ignores it.
        #[arg(long)]
        intent: Option<String>,
        /// The skill to check (`skill`).
        #[arg(long)]
        path: Option<PathBuf>,
        /// The declaration the question is about (`slice`, `users`,
        /// `history`).
        #[arg(long)]
        name: Option<String>,
        /// The pull request whose description `history` shows, first
        /// paragraph only.
        #[arg(long)]
        pr: Option<u32>,
        /// Any directory inside the project. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },

}

/// Dispatch one `scan`-family `run` subcommand.
pub fn dispatch(cmd: ScanCmd) {
    match cmd {
        ScanCmd::Scan { root, out, full } => scan::run(&root, out.as_deref(), full),
        ScanCmd::Map { question, file, task, query, intent, path, name, pr, root } => {
            crate::commands::map::run(&crate::commands::map::MapOpts {
                root,
                question,
                file,
                task,
                query,
                intent,
                path,
                name,
                pr,
                session: crate::shared::spec_state::session_from_env(),
            });
        }
    }
}
