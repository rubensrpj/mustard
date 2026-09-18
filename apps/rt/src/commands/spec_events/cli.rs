//! The `run` subcommands of the spec event file (`spec_events/`).
//!
//! A new command takes its variant in [`SpecEventsCmd`] and its arm in
//! [`dispatch`] below (the compiler demands the arm), its line in
//! `tests/fixtures/run-surface.txt`, which `tests/run_command_surface.rs`
//! compares with the clap tree, and a caller in the product text, which
//! `tests/template_parity.rs` demands with no exception list.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run read`, never `run spec-events read`.

use clap::Subcommand;
use mustard_core::domain::spec_events::TYPES;
use std::fmt::Write as _;
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
    #[command(display_order = 8)]
    Read {
        /// The block to read, e.g. `state` or `wave-2`.
        block: String,
        /// The spec whose file is read. Without it, the current spec: the
        /// `MUSTARD_ACTIVE_SPEC` override, then the spec of the checkout's
        /// branch, then the spec bound to the session; with none, refused.
        #[arg(long)]
        spec: Option<String>,
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
    #[command(display_order = 9, after_help = fields_of_each_type())]
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
    #[command(display_order = 14)]
    Index {
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
}

/// O fim da ajuda do `run write`: os campos de cada tipo, lidos da mesma
/// tabela que a conferência da gravação usa, então a ajuda nunca diverge do
/// que a gravação recusa. Os obrigatórios vêm primeiro, com o `origin` nos
/// tipos que o assistente grava a partir da conversa, e os que podem faltar
/// vêm entre parênteses.
fn fields_of_each_type() -> String {
    let mut help = String::from(
        "Fields of each type, the required ones first and the optional ones in parentheses. Every \
         type also takes `author`, `label` and `replaces`, and `text` and `keys` where not listed:",
    );
    for spec in TYPES {
        let named = |required: bool| spec.fields.iter().filter(move |f| f.required == required).map(|f| f.name);
        let mut required: Vec<&str> = named(true).collect();
        if spec.needs_origin {
            required.push("origin");
        }
        let optional: Vec<&str> = named(false).collect();
        let _ = write!(help, "\n  {}: {}", spec.name, required.join(", "));
        if !optional.is_empty() {
            let gap = if required.is_empty() { "" } else { " " };
            let _ = write!(help, "{gap}({})", optional.join(", "));
        }
    }
    help
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

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};
    use serde_json::{json, Value};

    use super::*;
    use crate::commands::spec_events::write::{record_open, seed_at, WriteOpts};

    /// O comando montado só para exercitar o parser da família.
    #[derive(Debug, Parser)]
    struct Harness {
        #[command(subcommand)]
        cmd: SpecEventsCmd,
    }

    /// A ajuda do comando de gravar lista os campos de cada tipo, um tipo por
    /// linha: os obrigatórios, com a origem nos tipos que o assistente grava,
    /// e os que podem faltar entre parênteses.
    #[test]
    fn the_write_help_lists_the_fields_of_each_type() {
        let mut tree = Harness::command();
        let write = tree.find_subcommand_mut("write").expect("the write command is registered");
        let help = write.render_long_help().to_string();
        for spec in TYPES {
            assert!(help.contains(&format!("\n  {}: ", spec.name)), "the help has no line for {}:\n{help}", spec.name);
        }
        assert!(help.contains("\n  decision: text, keys, why, origin (applies_to, waves, no_code)"), "{help}");
        assert!(help.contains("\n  message: text (witness)"), "{help}");
    }

    /// Uma decisão gravada pelo comando sem dois campos obrigatórios é
    /// recusada com os dois na recusa, e nada é gravado.
    #[test]
    fn a_decision_written_without_two_fields_is_refused_with_both() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        assert_eq!(record_open(root, "teste", "feature/teste", "dev"), Ok(true));
        let events = mustard_core::io::spec_events::spec_file(root, "teste").expect("spec file");
        let before = std::fs::read_to_string(&events).expect("events");
        let fields = json!({"text": "Gravar tudo.", "origin": 1}).to_string();
        let root_arg = root.to_string_lossy().into_owned();
        let args = ["x", "write", "decision", "--spec", "teste", "--json", &fields, "--root", &root_arg];
        let Harness { cmd: SpecEventsCmd::Write { event_type, spec, json, root } } =
            Harness::try_parse_from(args).expect("the write parses")
        else {
            panic!("not the write command");
        };
        let refused = seed_at(&WriteOpts { root, spec, event_type, json });
        assert_eq!(refused["reason"], json!("missing-field"), "{refused}");
        let hint = refused["hint"].as_str().unwrap_or_default();
        assert!(hint.contains("keys, why"), "{refused}");
        assert_eq!(std::fs::read_to_string(&events).expect("events"), before, "a refusal writes nothing");
        assert_eq!(refused.get("id"), None::<&Value>);
    }
}
