//! The `run` face of `mustard-rt` — the script port.
//!
//! `mustard-rt on` is the enforcement face: it reads the harness JSON from
//! stdin and runs the hooks. The `run` face is
//! different — it ports the utility *scripts* that used to live under
//! `templates/scripts/` as standalone `bun` programs. A `run` subcommand takes
//! its inputs as `clap` arguments (a directory, flags), never from stdin, and
//! prints its result to stdout exactly as the JS script did.
//!
//! Each ported script is its own submodule. (The early `sync-detect` /
//! `sync-registry` scanner ports were since removed — subproject discovery now
//! comes from the map `.claude/grain.db` the scan tool writes.)
//!
//! ## Layout — one clap enum per family, no god-enum
//!
//! [`RunCmd`] owns NO leaf command: it is a thin router of
//! `#[command(flatten)]` variants, one per family. Each family owns its own
//! `cli.rs` (`spec/cli.rs`, `flow/cli.rs`, …) holding BOTH its `…Cmd` enum and
//! the `dispatch()` arms that run it. `flatten` hoists the child subcommands to
//! THIS level, so every published name stays flat and unchanged:
//! `mustard-rt run pr-open`, never `mustard-rt run review pr-open`.
//!
//! THE INVARIANT, scoped per family: a new `run` subcommand takes the variant
//! in its family's enum and the arm in that family's `dispatch()` (the
//! compiler demands the arm). Two tests hold the rest:
//! `tests/run_command_surface.rs` compares the clap tree with
//! `tests/fixtures/run-surface.txt`, so a dropped registration (or an
//! accidental rename) fails CI instead of silently disappearing from the CLI
//! the hooks and the prose call; and `tests/template_parity.rs` refuses a
//! command that no prose or argv calls, with no exception list.

pub mod agent;
pub mod wave;
pub mod doctor;
pub mod review;
pub mod event;
pub mod spec;
pub mod maint;
pub mod git_delete;
pub mod git_settle;
pub mod scan;
pub mod scan_claude;
pub mod map;
pub mod spec_events;
pub mod flow;
pub mod retired;
pub mod statusline;
// Families whose commands are ported scripts living in flat modules (no
// `<family>/` directory of their own) keep their clap enum in a `*_cli.rs`
// sibling — same contract as a `<family>/cli.rs`.
pub mod scan_cli;

use clap::Subcommand;

/// The `run` subcommands — one flattened family per variant.
///
/// Every variant is `#[command(flatten)]`, so clap hoists the family's own
/// subcommands to the `run` level: the names the hooks, `settings.json` and the
/// skill templates call are unchanged and stay flat.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — clap-Subcommand; boxing breaks derive
pub enum RunCmd {
    /// Installation health: the closed check list the contract names.
    #[command(flatten)]
    Doctor(doctor::cli::DoctorCmd),
    /// A lista de pendências.
    #[command(flatten)]
    Event(event::cli::EventCmd),
    /// O fluxo da spec, um comando por passo.
    #[command(flatten)]
    Flow(flow::cli::FlowCmd),
    /// Instalação: a atualização e a faxina das cópias abandonadas.
    #[command(flatten)]
    Maint(maint::cli::MaintCmd),
    /// As portas do pull request: abrir, revisar e integrar.
    #[command(flatten)]
    Review(review::cli::ReviewCmd),
    /// The `/scan` chain: mine the repo model and enrich it.
    #[command(flatten)]
    Scan(scan_cli::ScanCmd),
    /// A porta das páginas avulsas.
    #[command(flatten)]
    Spec(spec::cli::SpecCmd),
    /// The spec event file: read one block, write one event.
    #[command(flatten)]
    SpecEvents(spec_events::cli::SpecEventsCmd),
    /// The Claude Code status bar.
    #[command(flatten)]
    Statusline(statusline::cli::StatuslineCmd),
}

/// Dispatch a `run` subcommand to its family.
///
/// Unlike the enforcement dispatcher this never touches stdin and never
/// produces an [`Outcome`](mustard_core::domain::model::contract::Outcome) — a `run`
/// script writes its own output and the process exits cleanly afterwards.
pub fn dispatch(cmd: RunCmd) {
    match cmd {
        RunCmd::Doctor(c) => doctor::cli::dispatch(c),
        RunCmd::Event(c) => event::cli::dispatch(c),
        RunCmd::Flow(c) => flow::cli::dispatch(c),
        RunCmd::Maint(c) => maint::cli::dispatch(c),
        RunCmd::Review(c) => review::cli::dispatch(c),
        RunCmd::Scan(c) => scan_cli::dispatch(c),
        RunCmd::Spec(c) => spec::cli::dispatch(c),
        RunCmd::SpecEvents(c) => spec_events::cli::dispatch(c),
        RunCmd::Statusline(c) => statusline::cli::dispatch(c),
    }
}

#[cfg(test)]
mod tests {
    use clap::{Command, CommandFactory};
    use mustard_core::platform::i18n::uppercase_words;

    /// A árvore inteira de `mustard-rt`, a mesma que o programa lê: o texto
    /// do programa, a porta `on` e cada comando de `run`.
    fn program_tree() -> Command {
        crate::cli::Cli::command()
    }

    /// As palavras em maiúsculas fora de crase em `texts`, cada uma uma vez,
    /// no defeito que diz onde ela está.
    fn push_defects(place: &str, texts: &[String], out: &mut Vec<String>) {
        let mut seen: Vec<&str> = Vec::new();
        for text in texts {
            for word in uppercase_words(text) {
                if !seen.contains(&word) {
                    seen.push(word);
                    out.push(format!("{place}: uppercase word {word} outside backticks"));
                }
            }
        }
    }

    /// Cada palavra em maiúsculas fora de crase na ajuda de `cmd` e dos
    /// comandos abaixo dele: o texto do comando, o de cada argumento e o de
    /// cada valor que o argumento aceita. O defeito diz o comando, o
    /// argumento, quando há, e a palavra.
    fn help_uppercase_defects(path: &str, cmd: &Command, out: &mut Vec<String>) {
        let own: Vec<String> = [
            cmd.get_about(),
            cmd.get_long_about(),
            cmd.get_before_help(),
            cmd.get_before_long_help(),
            cmd.get_after_help(),
            cmd.get_after_long_help(),
        ]
        .into_iter()
        .flatten()
        .map(ToString::to_string)
        .collect();
        push_defects(path, &own, out);
        for arg in cmd.get_arguments() {
            let name = arg.get_long().map_or_else(|| arg.get_id().to_string(), |long| format!("--{long}"));
            let mut texts: Vec<String> =
                [arg.get_help(), arg.get_long_help()].into_iter().flatten().map(ToString::to_string).collect();
            texts.extend(arg.get_possible_values().iter().filter_map(|value| value.get_help().map(ToString::to_string)));
            push_defects(&format!("{path} {name}"), &texts, out);
        }
        for sub in cmd.get_subcommands() {
            help_uppercase_defects(&format!("{path} {}", sub.get_name()), sub, out);
        }
    }

    /// Os defeitos da árvore inteira, a partir do nome do programa.
    fn tree_defects(tree: &Command) -> Vec<String> {
        let mut out = Vec::new();
        help_uppercase_defects(tree.get_name(), tree, &mut out);
        out
    }

    /// A ajuda do programa inteiro, da entrada ao último comando de `run`,
    /// segue a regra das frases do programa: nenhuma palavra toda em
    /// maiúsculas fora de crase, salvo a lista curta de siglas e unidades. A
    /// falha lista o comando, o argumento e a palavra.
    #[test]
    fn every_command_help_keeps_uppercase_inside_backticks() {
        let tree = program_tree();
        assert_eq!(tree.get_name(), "mustard-rt", "the walk starts at the program itself");
        assert!(tree.find_subcommand("on").is_some(), "the walk reaches the hook entry");
        let commands = tree.find_subcommand("run").map_or(0, |run| run.get_subcommands().count());
        assert!(commands >= 20, "the check reached the whole run tree: {commands} commands");
        let defects = tree_defects(&tree);
        assert!(defects.is_empty(), "{} help texts break the uppercase rule:\n{}", defects.len(), defects.join("\n"));
    }

    /// Um "THE" solto na ajuda de um comando, ou na de um argumento dele, cai
    /// com o comando, o argumento e a palavra; entre crases ele passa. Vale
    /// também para a entrada do programa: o texto dele, o de `on` e o do
    /// argumento de `on`.
    #[test]
    fn a_loose_uppercase_word_in_a_help_fails_naming_the_command() {
        let with = |about: &str, help: &str| {
            let (about, help) = (about.to_string(), help.to_string());
            program_tree().mut_subcommand("run", move |run| {
                run.mut_subcommand("pending", move |pending| {
                    pending.about(about).mut_arg("add", move |add| add.help(help))
                })
            })
        };
        assert_eq!(
            tree_defects(&with("Lists THE pending items.", "The item text.")),
            vec!["mustard-rt run pending: uppercase word THE outside backticks"]
        );
        assert_eq!(
            tree_defects(&with("Lists the pending items.", "Writes THE item.")),
            vec!["mustard-rt run pending --add: uppercase word THE outside backticks"]
        );
        assert_eq!(tree_defects(&with("Lists `THE` pending items.", "Writes `THE` item.")), Vec::<String>::new());

        let entry = |program: &str, on: &str, event: &str| {
            let (program, on, event) = (program.to_string(), on.to_string(), event.to_string());
            program_tree()
                .about(program)
                .mut_subcommand("on", move |cmd| cmd.about(on).mut_arg("event", move |arg| arg.help(event)))
        };
        assert_eq!(
            tree_defects(&entry("Mustard runtime.", "Runs THE hooks of an event.", "The event name.")),
            vec!["mustard-rt on: uppercase word THE outside backticks"]
        );
        assert_eq!(
            tree_defects(&entry("Mustard runtime.", "Runs the hooks of an event.", "THE event name.")),
            vec!["mustard-rt on event: uppercase word THE outside backticks"]
        );
        assert_eq!(
            tree_defects(&entry("Mustard RUNTIME.", "Runs the hooks of an event.", "The event name.")),
            vec!["mustard-rt: uppercase word RUNTIME outside backticks"]
        );
        assert_eq!(
            tree_defects(&entry("Mustard `RUNTIME`.", "Runs `THE` hooks.", "`THE` event name.")),
            Vec::<String>::new()
        );
    }
}
