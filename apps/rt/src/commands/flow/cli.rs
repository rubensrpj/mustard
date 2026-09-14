//! The `run` subcommands of the spec flow (`flow/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`FlowCmd`] AND its arm in [`dispatch`] below; forgetting the arm still
//! compiles, but the command vanishes from the CLI. The other two live in the
//! tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run open`, never `run flow open`.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::flow;

/// The `run` subcommands owned by the spec flow (`flow/`).
#[derive(Debug, Subcommand)]
pub enum FlowCmd {
    /// Open a spec: the branch `<kind>/<name>` and the spec `<name>`, born in
    /// the survey phase with its branch and base. The name is used exactly as
    /// written; what git refuses (a space, an accent, a slash) is adjusted and
    /// shown for a yes before anything is created. A missing kind, name or
    /// base is asked back as a step (`choose_kind`, `choose_name`,
    /// `choose_base`), with the candidates; nothing is created until all three
    /// are known. The answer ends with the goal question to ask the user.
    #[command(display_order = 104)]
    Open {
        /// The branch kind, such as `feature` or `fix`. Without it, a name
        /// written as `<kind>/<name>` is split into both.
        #[arg(long)]
        kind: Option<String>,
        /// The spec's name, exactly as the user wrote it. It names the branch
        /// `<kind>/<name>` and the spec folder.
        #[arg(long)]
        name: Option<String>,
        /// The branch the spec starts from. Without it, the answer lists the
        /// bases `git.flow` declares, or the repository's branches when it
        /// declares none.
        #[arg(long)]
        base: Option<String>,
        /// Any directory inside the repo. The branch is created in this
        /// checkout; the spec lives in the main one. Defaults to the current
        /// dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Survey a spec: record the work type and build the point list - the
    /// gaps of each kind (joined without repeats in a mixed request), the
    /// lessons and the earlier specs that match the goal, and up to three old
    /// user messages as reminders inside those points. The assistant records
    /// each point with `write point`; running grill again with the same kinds
    /// writes nothing and returns the first open point.
    #[command(display_order = 105)]
    Grill {
        /// The spec surveyed. Without it, the current spec.
        #[arg(long)]
        spec: Option<String>,
        /// The work type, comma-separated: `feature`, `fix`, `refactor`.
        #[arg(long)]
        kinds: Option<String>,
        /// A request that fits in one sentence: every point in one block,
        /// shown at once for a single yes.
        #[arg(long)]
        condensed: bool,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `flow`-family `run` subcommand.
pub fn dispatch(cmd: FlowCmd) {
    match cmd {
        FlowCmd::Open { kind, name, base, root } => {
            flow::open::run(&flow::open::OpenOpts { root, kind, name, base });
        }
        FlowCmd::Grill { spec, kinds, condensed, root } => {
            flow::grill::run(&flow::grill::GrillOpts { root, spec, kinds, condensed });
        }
    }
}
