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
    /// Retrieve current functions, documents, configuration and interpretations
    /// without a model call. Export a report with `--markdown --out <file>`.
    #[command(display_order = 28)]
    Knowledge {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = "")]
        query: String,
        #[arg(long)]
        file: Option<String>,
        /// Exact card identity returned by discovery: <file>:<line>:<name>.
        #[arg(long, conflicts_with_all = ["query", "file", "record", "refresh"])]
        symbol: Option<String>,
        /// Follow calls, consumers, or both from one exact symbol.
        #[arg(long, requires = "symbol", value_parser = ["outgoing", "callers", "both"])]
        direction: Option<String>,
        /// List stale interpretations and affected sources for explicit review.
        #[arg(long, conflicts_with_all = ["record", "symbol", "direction"])]
        refresh: bool,
        #[arg(long, default_value_t = 8)]
        limit: usize,
        #[arg(long, default_value_t = 2)]
        depth: usize,
        #[arg(long, conflicts_with = "record")]
        all: bool,
        #[arg(long, conflicts_with = "record")]
        markdown: bool,
        /// Expand stored evidence and every current relation of selected symbols.
        #[arg(long, conflicts_with = "record")]
        detail: bool,
        #[arg(long, conflicts_with = "record")]
        out: Option<PathBuf>,
        /// Explicit multi-source interpretation receipt, as a `.json` file.
        #[arg(long, conflicts_with_all = ["query", "file", "all", "markdown", "detail", "out", "symbol", "direction", "refresh"])]
        record: Option<PathBuf>,
    },
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
    /// (`--file <target>`), `importers --file`,
    /// `tests --file`, `slice --file --name <declaration>` (the declaration's
    /// own lines, without opening the file), `users --name <declaration>` (who
    /// uses it, as `file:line:caller`; `--file` keeps the one declared in that
    /// file), `history --name <declaration>` (the commits of the base branch
    /// that changed it, newest first, with title and pull request number;
    /// `--file` picks the file when the name lives in more than one), `search
    /// "<pattern>" [<folder>]` (the same text you would give `Grep`, with the
    /// options `Grep` and `grep` take: `-i`, `-w`, `-F`, `--glob` and
    /// `--type`; the answer is the one Mustard gives to that search), `summary` (the
    /// summary of the project map, up to 3 kB; with `--file`, the parts of that
    /// file: each declaration with its kind, name and lines, and the line
    /// where its tests start), `dump` (the map database table by
    /// table, in a fixed order, for debugging) or `note "<sentence>" --file
    /// <file> [--name <declaration>]` (writes the one-sentence meaning of that
    /// file, or declaration, in business words, so the search finds it by
    /// them; it stays valid until the file changes, and `slice` shows it, as
    /// stale once the file changed). Reads `.claude/grain.db`;
    /// prints JSON and exits 1 on a refusal.
    #[command(display_order = 16)]
    Map {
        /// The question to ask.
        #[arg(value_enum)]
        question: crate::commands::map::Question,
        /// The text to look for (`search`), the same you would give `Grep`:
        /// a regular expression, or plain text with `-F`; for `note`, the
        /// sentence of what the file or declaration is for.
        #[arg(value_name = "PATTERN")]
        pattern: Option<String>,
        /// The folder to look in (`search`); the current one by default.
        #[arg(value_name = "FOLDER")]
        folder: Option<PathBuf>,
        /// Ignore case (`search`).
        #[arg(short = 'i', long = "ignore-case")]
        ignore_case: bool,
        /// Match whole words only (`search`).
        #[arg(short = 'w', long = "word-regexp")]
        whole_word: bool,
        /// Read the pattern as plain text (`search`).
        #[arg(short = 'F', long = "fixed-strings")]
        fixed: bool,
        /// Only the files whose name matches, as the `glob` of `Grep`; a
        /// leading `!` leaves them out (`search`).
        #[arg(long)]
        glob: Option<String>,
        /// Only the files of this type, as the `type` of `Grep` (`search`).
        #[arg(long = "type", value_name = "TYPE")]
        kind: Option<String>,
        /// The file the question is about (for `examples`, the file the task
        /// creates or changes, or its folder; for `users` and `history`,
        /// optional, keeps the declaration of that file; for `summary`,
        /// optional, lists the parts of that file).
        #[arg(long)]
        file: Option<String>,
        /// The words to look for (`search`); an alias kept for the
        /// measurements of the search, hidden from the help.
        #[arg(long, hide = true)]
        query: Option<String>,
        /// The sentence of what is looked for and why (`search`); an alias
        /// kept for the measurements of the search, hidden from the help.
        #[arg(long, hide = true)]
        intent: Option<String>,
        /// The description the agent gave the search (`search`); the terminal
        /// hook fills it by itself, so it is only an option for the
        /// measurements of the search, hidden from the help.
        #[arg(long, hide = true)]
        described: Option<String>,
        /// The last thing the agent said before the search (`search`); the
        /// terminal hook fills it by itself, so it is only an option for the
        /// measurements of the search, hidden from the help.
        #[arg(long, hide = true)]
        said: Option<String>,
        /// The declaration the question is about (`slice`, `users`,
        /// `history`, `note`).
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
        ScanCmd::Knowledge { root, query, file, symbol, direction, refresh, limit, depth, all, markdown, detail, out, record } => {
            super::knowledge::run(
                &root,
                &mustard_core::io::knowledge::Query {
                    text: &query,
                    file: file.as_deref(),
                    symbol: symbol.as_deref(),
                    refresh,
                    limit,
                    depth,
                    all,
                    detail: detail || markdown,
                    direction: match direction.as_deref() {
                        Some("callers") => mustard_core::io::knowledge::Direction::Callers,
                        Some("both") => mustard_core::io::knowledge::Direction::Both,
                        _ => mustard_core::io::knowledge::Direction::Outgoing,
                    },
                },
                markdown,
                out.as_deref(),
                record.as_deref(),
            );
        }
        ScanCmd::Scan { root, out, full } => scan::run(&root, out.as_deref(), full),
        map @ ScanCmd::Map { .. } => crate::commands::map::run(&map_opts(map)),
    }
}

/// The options of `run map` that the command line carries, every flag in its
/// field. Only called with the `Map` command.
fn map_opts(cmd: ScanCmd) -> crate::commands::map::MapOpts {
    let ScanCmd::Map {
        question,
        pattern,
        folder,
        ignore_case,
        whole_word,
        fixed,
        glob,
        kind,
        file,
        query,
        intent,
        described,
        said,
        name,
        pr,
        root,
    } = cmd
    else {
        unreachable!("the options of `run map` are read from the `Map` command only")
    };
    let grep = pattern.map(|pattern| crate::commands::map::GrepSearch {
        pattern,
        folder,
        ignore_case,
        whole_word,
        fixed,
        glob,
        kind,
    });
    crate::commands::map::MapOpts {
        root,
        question,
        grep,
        file,
        query,
        intent,
        described,
        said,
        name,
        pr,
        session: crate::shared::spec_state::session_from_env(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Probe {
        #[command(subcommand)]
        cmd: ScanCmd,
    }

    fn opts_of(args: &[&str]) -> crate::commands::map::MapOpts {
        let mut line = vec!["probe", "map"];
        line.extend_from_slice(args);
        map_opts(Probe::try_parse_from(line).expect("the command line parses").cmd)
    }

    /// `--described` e `--said` da linha de comando chegam às opções da busca,
    /// com `--query` e com o texto do `Grep`, e sem eles ficam vazios.
    #[test]
    fn the_description_and_the_speech_of_the_command_line_reach_the_search_options() {
        let with_query = opts_of(&["search", "--query", "frete", "--described", "Procura o frete", "--said", "Vou olhar"]);
        assert_eq!(with_query.query.as_deref(), Some("frete"));
        assert_eq!(with_query.described.as_deref(), Some("Procura o frete"));
        assert_eq!(with_query.said.as_deref(), Some("Vou olhar"));

        let with_pattern = opts_of(&["search", "frete", ".", "--described", "Procura o frete", "--said", "Vou olhar"]);
        assert_eq!(with_pattern.grep.as_ref().map(|grep| grep.pattern.as_str()), Some("frete"));
        assert_eq!(with_pattern.described.as_deref(), Some("Procura o frete"));
        assert_eq!(with_pattern.said.as_deref(), Some("Vou olhar"));

        let bare = opts_of(&["search", "--query", "frete"]);
        assert_eq!((bare.described, bare.said), (None, None));
    }

    /// `examples` só recebe o alvo pelo `--file`: o texto de uma tarefa na
    /// linha de comando não vira pasta, e a linha com o alvo segue valendo.
    #[test]
    fn examples_takes_the_target_by_file_and_never_a_task_text() {
        let refused = Probe::try_parse_from(["probe", "map", "examples", "--task", "adicionar um comando run"]);
        assert!(refused.is_err(), "a task text is no longer an option of the question");
        let by_file = opts_of(&["examples", "--file", "apps/rt/src/commands/pay"]);
        assert_eq!(by_file.file.as_deref(), Some("apps/rt/src/commands/pay"));
    }
}
