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
use mustard_core::domain::spec_events::{TYPES, TypeSpec, type_spec};
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::commands::spec_events;

/// The `run` subcommands owned by the spec event file (`spec_events/`).
#[derive(Debug, Subcommand)]
pub enum SpecEventsCmd {
    /// Read one block of a spec's event file (`.claude/spec/<spec>/spec.ndjson`
    /// of the main checkout), never the whole file: `state`, `metrics`,
    /// `agreed`, `specification`, `criteria`, `waves`, `wave-<n>`, `review`,
    /// `progress`, `notes` or `conversation`. Removed and replaced items are
    /// left out; a line that does not parse is skipped with a warning. A
    /// block but `state` comes newest first, up to 30 events without `--all`.
    /// `wave-list` lists one short line per open wave, by number: its code, the
    /// first line of its text and the waves it depends on. `dispatch-<n>` reads, in one go, every item the request of wave `n`
    /// lists, each with its code, and its lessons: the list the send recorded.
    /// `dispatch-review` does the same for the newest final review request.
    /// `request-<n>` prints, as plain text, the
    /// exact request recorded when wave `n` went out: the agent reads its own
    /// request this way; a wave that never went out is refused. `request-review` does the same for the final
    /// review, which has no wave: the close answer hands the reviewer this
    /// command instead of the request. `request-review-preview` prints, as
    /// plain text, the final review request the close would build now,
    /// recording nothing: the user sees the real request before it counts.
    /// `delivered-<n>` reads the wave's
    /// current delivery (the one the round took over, else the agent's own),
    /// keeping only the agreed items not met. `backlog` lists the tasks not yet delivered,
    /// each with its code, current version, wave, file count and the pending
    /// tasks it waits on, then the totals. `calls` sums the calls of each
    /// command, one line per command: how many, the failures by reason, the
    /// median, p90 and worst of the call time and of the filter time, and
    /// the tokens and cost added up; `--term` names the one command.
    /// `item-MSTD-TASK-NNNN` reads the item's current version and `item-<n>`
    /// that version, each with `changed` (what changed from the previous
    /// version); a removed item says
    /// `removed`, and a user message shows its whole text like any other item.
    /// The `state` block also gives `last_user_message`, the number of the
    /// user's latest message, without its text. Reading an item, a lesson
    /// (`lessons --term <n>`), `dispatch-<n>` or `dispatch-review` from inside the copy of an
    /// open request is recorded as read: the delivery of a wave request, and
    /// the verdict of the final review, are refused while an item the
    /// request lists was not read. From inside that copy an item line comes
    /// without `v`, `at`, `author` and `keys` (a user message keeps `at` and
    /// `author`). `metrics` also lists the hook actions, the injections and
    /// the command calls. They live in the spec's operations file, or in the
    /// spec itself in older specs, and `calls` sums both. Read without
    /// `--term`, `metrics` lists the send, the injection and the delivery
    /// without their `text`: `--term` with `--all` brings them whole, the
    /// item code brings one whole, and `delivered-<n>` reads the delivery.
    #[command(display_order = 8)]
    Read {
        /// The block to read, e.g. `state`, `wave-list`, `wave-2`, `dispatch-2`,
        /// `request-2`, `request-review`, `request-review-preview`,
        /// `delivered-2`, `backlog`, `calls` or
        /// `item-MSTD-TASK-0003`.
        block: String,
        /// The spec whose file is read. Without it, the current spec: the
        /// `MUSTARD_ACTIVE_SPEC` override, then the spec of the checkout's
        /// branch, then the spec bound to the session; with none, refused.
        #[arg(long)]
        spec: Option<String>,
        /// Keep only the events whose words or item code match this term —
        /// the conversation searched for a subject, an item found by the
        /// code the page shows, like `MSTD-CRIT-NNNN`, or the calls of a
        /// command by its name, like `map search`.
        #[arg(long)]
        term: Option<String>,
        /// The whole answer. Without it, `backlog` opens with the total and
        /// leaves out of each line the empty wave and the empty dependency
        /// list; the other blocks read the same either way.
        #[arg(long)]
        full: bool,
        /// Everything the reading finds. Without it, a block shows its newest
        /// 30 events, with `more` saying how many of how many; `wave-list`
        /// shows only the open waves; and `metrics` with a `--term` that is
        /// not an item code shows a summary by type, with the newest codes.
        #[arg(long)]
        all: bool,
        /// Only the records of this Claude Code session, in `calls` and
        /// `metrics`: two conversations in the same project write to the same
        /// spec, and their usage numbers added up as one.
        #[arg(long)]
        session: Option<String>,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Write one event to a spec's event file, the only way it is written.
    /// Refuses an unknown type, an empty required field, a `task` missing one
    /// of its three mandatory declarations (what it does, the files it
    /// touches, which tasks it depends on) and, on a `point`, a fact without
    /// a source or citing a file that does not exist. `remove`,
    /// `purge` and a new version (`replaces`) are events like any other; they
    /// point at an item by its event number or by the code the page shows,
    /// like `MSTD-RULE-NNNN`. A `remove` by the code takes out the whole
    /// item, every version of it; by the number, only that version, and the
    /// version it replaced comes back, except on a `task`, where any version
    /// takes out the whole task. A new agreed item written here comes
    /// in three parts: a short `title` (the item's own name, up to 70
    /// characters — not the 60 of a commit or pull request title, which is
    /// built from the goal's first sentence and is another thing), the user's part
    /// in `text`, which says why by the effect the user sees and carries no
    /// backtick, file path or item code, and the agent's part in `agent`, in
    /// markdown, with files, lines, commands and what to test. A `request` or
    /// `note` needs the title and the text, a `criterion` only the title. The
    /// title and the user's part go through the writing check that ends a
    /// response, and the refusal lists every defect at once. A new version of
    /// an item written before this form, or a write by the program itself,
    /// is not held to it. With the `lesson` type it writes one lesson to
    /// the lesson bank (`.claude/spec/lessons.ndjson`) instead:
    /// `{"class":"environment_trap","text":"…","keys":["…"],"applies_to":{"subproject":"…"},"found_in":{"spec":"…"}}`;
    /// a lesson valid everywhere says `"applies_to":{"files":["**"]}`. A
    /// `defect` or `project_rule` lesson is refused, alone or merging others:
    /// the bank never goes to git, so a defect that can happen again becomes a
    /// fix in the code, with the test that fails if it comes back, and a
    /// project rule becomes a test that fails if the rule is broken; with a
    /// spec open, the refusal names the `task` to write in it. A
    /// lesson is the assistant's summary, written the project's way: its text
    /// goes through the writing check that ends a response, and a text that
    /// repeats a lesson already in the bank (spaces, case and accents aside)
    /// is refused, naming that lesson. A lesson with `"replaces":[3,5]` merges
    /// those lessons into one; `{"targets":[4],"reason":"…"}`, with no
    /// `class` and no other field, retires lesson 4. The answer to a survey
    /// point goes through `answer` instead, which records the item and
    /// closes the point in one call.
    #[command(display_order = 9, after_help = fields_of_each_type())]
    Write {
        /// The event type, e.g. `rule`, `decision`, `wave`, `remove` or
        /// `lesson`.
        #[arg(value_name = "TYPE")]
        event_type: String,
        /// The spec whose file receives the event. Required for every type
        /// but `lesson`, which takes it, when given, as the spec the lesson
        /// was found in, and the project page's `publish`, which without it
        /// records the address straight on the project line of the spec
        /// index: the page is born before any spec exists.
        #[arg(long)]
        spec: Option<String>,
        /// The event's own fields as one JSON object, e.g.
        /// `{"text":"…","keys":["…"],"example":"…","origin":3}`. The binary
        /// sets `v`, `id`, `code`, `at` and `search`; a `code` sent here is
        /// refused. An item of a type that needs `origin` and comes without
        /// it takes the number of the user's latest message; with no user
        /// message in the spec, the missing field is refused.
        #[arg(long, default_value = "{}")]
        json: String,
        /// Kept only so older calls still run: no write prepares a copy of
        /// the page anymore, and the answer says to publish explicitly with
        /// `/mustard:pages`. The write itself is the same with or without it.
        #[arg(long)]
        copy: bool,
        /// Any directory inside the repo. Defaults to the current dir.
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Find items in a spec in one call, as plain text: one line per item with
    /// its code, its type, its wave, its day and its title, at most 5 lines.
    /// A text finds the items that have all its words (function words left
    /// out, each word by its root), the title weighing more than the text;
    /// with more than 5 such items, or none, up to 30 candidates go to Jev in
    /// one question and only what it judges useful comes back, and without
    /// Jev the score order comes with partial items marked and a line saying
    /// so. An item code prints the whole item, as reading the item does.
    /// `--type` alone lists the newest items of that type. Hook notices,
    /// call records, receipts and copies are never searched.
    #[command(display_order = 42)]
    Find {
        /// The text to find, or an item code like `MSTD-TASK-NNNN`.
        term: Option<String>,
        /// Only items of this type: the word the answer shows (`tarefa`,
        /// `task`, `mensagem`) or the type name. Repeat it for more types.
        #[arg(long = "type", value_name = "TYPE")]
        types: Vec<String>,
        /// The spec to search. Without it, the current spec, as in `read`.
        #[arg(long)]
        spec: Option<String>,
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
        help.push_str(&fields_line(spec));
    }
    help
}

/// A linha de um tipo no fim da ajuda: `\n  <tipo>: ` e os obrigatórios,
/// com o `origin` nos tipos que o assistente grava a partir da conversa, e os
/// que podem faltar entre parênteses.
fn fields_line(spec: &TypeSpec) -> String {
    let named = |required: bool| spec.fields.iter().filter(move |f| f.required == required).map(|f| f.name);
    let mut required: Vec<&str> = named(true).collect();
    if spec.needs_origin {
        required.push("origin");
    }
    let optional: Vec<&str> = named(false).collect();
    let mut line = format!("\n  {}: {}", spec.name, required.join(", "));
    if !optional.is_empty() {
        let gap = if required.is_empty() { "" } else { " " };
        let _ = write!(line, "{gap}({})", optional.join(", "));
    }
    line
}

/// A ajuda de `run write <TYPE> --help` (`args` é a linha de comando
/// inteira) com um tipo que existe: o uso e a linha dos campos só desse
/// tipo, da mesma tabela da ajuda inteira. O agente que só queria o formato
/// de um tipo recebia os campos de todos, dezenas de vezes por obra. `None`
/// sem tipo, com um tipo que não existe ou noutro comando: a ajuda inteira.
#[must_use]
pub fn write_help_for_type(args: &[String]) -> Option<String> {
    let rest = match args {
        [_, run, write, rest @ ..] if run == "run" && write == "write" => rest,
        _ => return None,
    };
    if !rest.iter().any(|arg| arg == "--help" || arg == "-h") {
        return None;
    }
    let mut words = rest.iter();
    let mut named = None;
    while let Some(arg) = words.next() {
        match arg.as_str() {
            "--spec" | "--json" | "--root" => {
                words.next();
            }
            flag if flag.starts_with('-') => {}
            word => {
                named = Some(word);
                break;
            }
        }
    }
    let spec = type_spec(named?)?;
    Some(format!(
        "Usage: mustard-rt run write {name} [--spec <SPEC>] [--json <JSON>] [--root <ROOT>]\n\n\
         Fields of `{name}`, the required ones first and the optional ones in parentheses. It also \
         takes `author`, `label` and `replaces`, and `text` and `keys` when not listed:{line}\n\n\
         `mustard-rt run write --help` shows the whole help.\n",
        name = spec.name,
        line = fields_line(spec),
    ))
}

/// Dispatch one `spec_events`-family `run` subcommand.
pub fn dispatch(cmd: SpecEventsCmd) {
    match cmd {
        SpecEventsCmd::Read { block, spec, term, full, all, session, root } => {
            spec_events::read::run(&spec_events::read::ReadOpts { root, spec, block, term, session }, full, all);
        }
        SpecEventsCmd::Write { event_type, spec, json, copy, root } => {
            spec_events::write::run(&spec_events::write::WriteOpts { root, spec, event_type, json }, copy);
        }
        SpecEventsCmd::Find { term, types, spec, root } => {
            spec_events::find::run(&spec_events::find::FindOpts { root, spec, term, types });
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

    /// A ajuda do comando de ler diz que a mensagem do usuário sai com o
    /// texto inteiro e que a leitura de dentro da cópia de um pedido aberto
    /// fica registrada; a frase antiga, que escondia o texto, não volta.
    #[test]
    fn the_read_help_says_a_message_shows_its_text_and_the_reading_is_recorded() {
        let mut tree = Harness::command();
        let read = tree.find_subcommand_mut("read").expect("the read command is registered");
        let help = read.render_long_help().to_string();
        let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(flat.contains("a user message shows its whole text like any other item"), "{help}");
        assert!(flat.contains("is recorded as read"), "{help}");
        assert!(!flat.contains("shows only its number and type"), "{help}");
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
        assert!(help.contains("\n  decision: text, keys, why, origin (applies_to, waves, every_wave, no_code, title, agent)"), "{help}");
        assert!(help.contains("\n  message: text (witness, during)"), "{help}");
        assert!(
            help.contains("a `task` missing one of its three mandatory declarations"),
            "the help does not name the task-declaration refusal:\n{help}"
        );
    }

    /// A ajuda pedida com o tipo — `run write step --help` — mostra só o uso
    /// do comando e a linha dos campos desse tipo, não a ajuda inteira, mesmo
    /// com as opções antes do tipo. Sem tipo, com um tipo que não existe ou
    /// noutro comando, nada: a ajuda inteira continua.
    #[test]
    fn the_write_help_with_a_type_shows_only_that_type() {
        let args = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();
        let help = write_help_for_type(&args(&["mustard-rt", "run", "write", "step", "--help"])).expect("the help of one type");
        assert!(help.contains("Usage: mustard-rt run write"), "{help}");
        assert!(help.contains("\n  step: wave, item, text"), "{help}");
        assert!(!help.contains("\n  delivered: ") && !help.contains("three mandatory declarations"), "{help}");
        let later = write_help_for_type(&args(&["mustard-rt", "run", "write", "--spec", "x", "--root=.", "delivered", "-h"])).expect("the help of one type");
        assert!(later.contains("\n  delivered: wave, text ("), "{later}");
        assert!(!later.contains("\n  step: "), "{later}");
        for whole in [
            args(&["mustard-rt", "run", "write", "--help"]),
            args(&["mustard-rt", "run", "write", "--spec", "step", "--help"]),
            args(&["mustard-rt", "run", "write", "nothing", "--help"]),
            args(&["mustard-rt", "run", "read", "step", "--help"]),
        ] {
            assert_eq!(write_help_for_type(&whole), None, "{whole:?}");
        }
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
        let Harness { cmd: SpecEventsCmd::Write { event_type, spec, json, root, .. } } =
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

#[cfg(test)]
mod title_limits_tests {
    use clap::{Parser, Subcommand};

    /// O comando montado só para ler a ajuda da família.
    #[derive(Debug, Parser)]
    struct Harness {
        #[command(subcommand)]
        cmd: super::SpecEventsCmd,
    }

    /// A ajuda do `write` diz de que é cada teto de título: o de 70 é do nome
    /// do item, e o de 60 é do título de um commit e de um pull request,
    /// montado da primeira frase do objetivo. Quem lia a ajuda achava que era
    /// o mesmo teto com dois valores.
    #[test]
    fn the_help_says_what_each_title_limit_is_of() {
        let tree = super::SpecEventsCmd::augment_subcommands(clap::Command::new("run"));
        let write = tree.find_subcommand("write").expect("o comando write");
        let said = write.get_about().map(ToString::to_string).unwrap_or_default();
        assert!(said.contains("70"), "{said}");
        assert!(said.contains("60"), "a ajuda cita os dois tetos: {said}");
        let at = said.find("70").expect("o teto do item");
        let around = &said[at.saturating_sub(60)..(at + 200).min(said.len())];
        assert!(around.contains("item"), "o de 70 é do item: {around}");
        assert!(around.contains("goal") || around.contains("pull request"), "e diz do que é o outro: {around}");
    }
}
