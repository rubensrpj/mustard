//! The `run` subcommands of the spec event file (`spec_events/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`SpecEventsCmd`] AND its arm in [`dispatch`] below; forgetting the arm
//! still compiles, but the command vanishes from the CLI. The other two live in
//! the tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run read`, never `run spec-events read`.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::spec_events;

/// The `run` subcommands owned by the spec event file (`spec_events/`).
#[derive(Debug, Subcommand)]
pub enum SpecEventsCmd {
    /// Read ONE block of a spec's event file (`.claude/spec/<spec>/spec.ndjson`
    /// of the main checkout), never the whole file: `state`, `metrics`,
    /// `agreed`, `specification`, `criteria`, `waves`, `wave-<n>`, `review`,
    /// `progress`, `notes` or `conversation`. Removed and replaced items are
    /// left out; a line that does not parse is skipped with a warning.
    #[command(display_order = 102)]
    Read {
        /// The block to read, e.g. `state` or `wave-2`.
        block: String,
        /// The spec whose file is read.
        #[arg(long)]
        spec: String,
        /// Keep only the events whose words or item code match this term —
        /// the conversation searched for a subject, or an item found by the
        /// code the page shows, like `MSTD-CRIT-0016`.
        #[arg(long)]
        term: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Write ONE event to a spec's event file, the only way it is written.
    /// Refuses an unknown type, an empty required field and, on a `point`, a
    /// fact without a source or citing a file that does not exist. `remove`,
    /// `purge` and a new version (`replaces`) are events like any other; they
    /// point at an item by its event number or by the code the page shows,
    /// like `MSTD-RULE-0002`. With the `lesson` type it writes one lesson to
    /// the lesson bank (`.claude/spec/lessons.ndjson`) instead:
    /// `{"class":"defect","text":"…","keys":["…"],"applies_to":{"subproject":"…"},"found_in":{"spec":"…"}}`;
    /// a lesson valid everywhere says `"applies_to":{"files":["**"]}`.
    #[command(display_order = 103)]
    Write {
        /// The event type, e.g. `rule`, `decision`, `wave`, `remove` or
        /// `lesson`.
        #[arg(value_name = "TYPE")]
        event_type: String,
        /// The spec whose file receives the event. Required for every type
        /// but `lesson`, which takes it, when given, as the spec the lesson
        /// was found in.
        #[arg(long)]
        spec: Option<String>,
        /// The event's own fields as one JSON object, e.g.
        /// `{"text":"…","keys":["…"],"example":"…","origin":3}`. The binary
        /// sets `v`, `id`, `code`, `at` and `search`; a `code` sent here is
        /// refused.
        #[arg(long, default_value = "{}")]
        json: String,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Rebuild the spec index (`.claude/spec/index.ndjson` of the main
    /// checkout) from every spec's event file, and recompute the `search`
    /// field of each line, when the index is missing or diverges. Every
    /// `write` already refreshes its own spec's line; this is the full repair
    /// the `doctor` names when it flags a divergence. Folders without an event
    /// file are listed in `skipped`.
    #[command(display_order = 60)]
    Index {
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// Dispatch one `spec_events`-family `run` subcommand.
pub fn dispatch(cmd: SpecEventsCmd) {
    match cmd {
        SpecEventsCmd::Read { block, spec, term, root } => {
            spec_events::read::run(&spec_events::read::ReadOpts { root, spec, block, term });
        }
        SpecEventsCmd::Write { event_type, spec, json, root } => {
            spec_events::write::run(&spec_events::write::WriteOpts { root, spec, event_type, json });
        }
        SpecEventsCmd::Index { root } => {
            spec_events::index::run(&spec_events::index::IndexOpts { root });
        }
    }
}
