//! The `run` subcommands for the harness event stream (`event/`).
//!
//! A new command takes its variant in [`EventCmd`] and its arm in
//! [`dispatch`] below (the compiler demands the arm), its line in
//! `tests/fixtures/run-surface.txt`, which `tests/run_command_surface.rs`
//! compares with the clap tree, and a caller in the product text, which
//! `tests/template_parity.rs` demands with no exception list.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run event <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{event};

/// The `run` subcommands owned by the harness event stream (`event/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum EventCmd {
    /// The PENDING ledger: work agreed in the conversation that has not closed
    /// yet. It lives OUTSIDE every unit, in `.claude/pending/ledger.json` of the
    /// main checkout, so it is recorded with no unit open, survives a branch
    /// switch and outlives the unit that delivers it. Without a flag it LISTS
    /// `{ok, open, closed, count_line}`; `--add` records one item and prints
    /// its `P-{n}` id; `--close` settles one as delivered, always with a
    /// non-blank `--reason`. A removal (`--remove`, `--drop`) takes two calls:
    /// the first shows what would leave and prints a code, the second passes
    /// that code in `--confirm` after the user's yes. `--stale` shows the idle
    /// items once, and `--expire --keep` drops the ones the user did not keep.
    #[command(display_order = 13)]
    Pending {
        /// Record a new item (needs `--title` and `--detail`).
        #[arg(long, conflicts_with_all = ["close", "drop"])]
        add: bool,
        /// What was agreed, one line.
        #[arg(long)]
        title: Option<String>,
        /// Its scope or reason, one line.
        #[arg(long)]
        detail: Option<String>,
        /// Settle the item `P-{n}` as DELIVERED.
        #[arg(long, value_name = "ID", conflicts_with = "drop")]
        close: Option<String>,
        /// Drop the item `P-{n}` ON PURPOSE: the same removal as `--remove
        /// --id P-{n}`, shown first and confirmed with `--confirm`.
        #[arg(long, value_name = "ID", group = "removal")]
        drop: Option<String>,
        /// Why the item leaves the list. Required by `--close`, `--drop` and
        /// `--remove`; a blank one is refused and nothing is written.
        #[arg(long)]
        reason: Option<String>,
        /// Take items out, with ONE selector (`--id`, `--term` or `--before`)
        /// and a `--reason`. Without `--confirm` it only shows what would
        /// leave and prints the code to confirm with.
        #[arg(long, group = "removal")]
        remove: bool,
        /// Removal selector: the items `P-{n}`, comma-separated.
        #[arg(long, value_name = "IDS", requires = "remove")]
        id: Option<String>,
        /// Removal selector: words searched in the title and the detail.
        #[arg(long, requires = "remove")]
        term: Option<String>,
        /// Removal selector: the items recorded before this day, `YYYY-MM-DD`.
        #[arg(long, value_name = "DAY", requires = "remove")]
        before: Option<String>,
        /// The code a removal preview printed: removes exactly that set, and
        /// nothing when the list changed since.
        #[arg(long, value_name = "CODE", requires = "removal")]
        confirm: Option<String>,
        /// Bring a dropped item `P-{n}` back to open.
        #[arg(long, value_name = "ID")]
        reopen: Option<String>,
        /// Show, once, the open items idle for 30 days or more, with the one
        /// question to ask the user.
        #[arg(long)]
        stale: bool,
        /// Drop as expired the idle items the last `--stale` showed, all but
        /// the `--keep` ones.
        #[arg(long)]
        expire: bool,
        /// The idle items that stay, comma-separated.
        #[arg(long, value_name = "IDS", requires = "expire")]
        keep: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `event`-family `run` subcommand.
pub fn dispatch(cmd: EventCmd) {
    match cmd {
        EventCmd::Pending {
            add,
            title,
            detail,
            close,
            drop,
            reason,
            remove,
            id,
            term,
            before,
            confirm,
            reopen,
            stale,
            expire,
            keep,
            root,
        } => {
            event::pending::run(&event::pending::PendingOpts {
                root,
                add,
                title,
                detail,
                close,
                drop,
                reason,
                now: None,
                remove,
                id,
                term,
                before,
                confirm,
                reopen,
                stale,
                expire,
                keep,
            });
        }
    }
}
