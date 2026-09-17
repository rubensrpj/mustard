//! The `run` subcommands for installation maintenance (`maint/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`MaintCmd`] AND its arm in [`dispatch`] below; forgetting the arm still
//! compiles, but the command vanishes from the CLI. The other two live in
//! the tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run maint <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{maint};

/// The `run` subcommands owned by installation maintenance (`maint/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum MaintCmd {
    /// Recolhe as cópias descartáveis que os agentes deixam no diretório
    /// temporário (ou no `scratchpad/` de uma sessão do Claude Code): pasta
    /// com cópia deste projeto ou `target/` de compilação, sem mudança há
    /// mais de 12 horas, e que não é a da sessão atual.
    ///
    /// Só lista por padrão; `--apply` apaga as listadas e esvazia a
    /// compilação compartilhada `~/.cache/mustard/scratch-target` acima de
    /// 8 GB. `--path <dir>` apaga uma pasta só, sem o filtro de idade, depois
    /// de conferir que ela está no temp e é uma cópia — fora do temp é
    /// recusado (exit 1). A exclusão é do próprio binário, nunca de shell.
    #[command(name = "clean")]
    #[command(display_order = 18)]
    ScratchGc {
        /// Só lista, sem apagar nada (o padrão). Não combina com `--apply`
        /// nem com `--path`: pedir para só listar e apontar uma pasta para
        /// apagar é contraditório, e a exclusão não tem volta — o parser
        /// recusa a chamada (exit 2) antes de qualquer coisa ser tocada.
        #[arg(long, default_value_t = true, conflicts_with_all = ["apply", "path"])]
        dry_run: bool,
        /// Apaga as candidatas listadas. Obrigatório para mexer no disco.
        #[arg(long)]
        apply: bool,
        /// Apaga só esta pasta, conferida, sem o filtro de idade. Não combina
        /// com `--apply`: são dois modos, e um calado pelo outro engana.
        #[arg(long, conflicts_with = "apply")]
        path: Option<PathBuf>,
    },
    /// Install or update Mustard in the current project (the plugin's
    /// bootstrap door).
    ///
    /// Idempotent. The settings file — `.claude/settings.local.json`, since
    /// the install is always private-mode and never touches the shared
    /// `.claude/settings.json` — plus `.claude/.gitignore` and the
    /// project-root `mustard.json` are yours and are merged, never clobbered:
    /// an existing file is preserved, only what is missing is created or
    /// backfilled. The three injectable instruction files under
    /// `.claude/mustard/` — `orchestrator.md`, `dispatch.md` and
    /// `material.md` — are ALWAYS rewritten: they are the harness's own
    /// rules, not project configuration, so a copy you edited is replaced and
    /// listed under `updated`, while one that already matched the shipped
    /// text comes back under `preserved` because there was nothing left to
    /// write. The legacy planted-orchestrator footprint is migrated away.
    /// Emits the `UpsertReport` as deterministic pretty JSON.
    #[command(display_order = 19)]
    Upsert {},
}

/// Dispatch one `maint`-family `run` subcommand.
pub fn dispatch(cmd: MaintCmd) {
    match cmd {
        MaintCmd::ScratchGc { dry_run, apply, path } => {
            // `dry_run` vale `true` por padrão e o `conflicts_with_all` recusa
            // `--dry-run` junto de `--apply` OU de `--path`: quando um dos dois
            // chega aqui, `dry_run` é só o padrão, nunca um pedido explícito.
            // Por isso descartá-lo é seguro — quem decide é `--apply`/`--path`.
            let _ = dry_run;
            maint::scratch_gc::run(maint::scratch_gc::ScratchGcOpts { apply, path });
        }
        MaintCmd::Upsert {} => maint::upsert::run(),
    }
}

#[cfg(test)]
mod tests {
    use super::MaintCmd;
    use clap::Parser;

    /// A wrapper so the family enum can be parsed on its own — the binary's own
    /// `Cli` lives in `main.rs` and is out of reach from the lib.
    #[derive(Parser)]
    struct Probe {
        #[command(subcommand)]
        cmd: MaintCmd,
    }

    /// `--dry-run --path X` apagava a pasta: o `dry_run` só conflitava com
    /// `--apply` e o dispatch o descarta. Agora o parser recusa a combinação,
    /// e o descarte no dispatch só vê o valor padrão.
    #[test]
    fn scratch_gc_dry_run_conflicts_with_path_and_apply() {
        let parse = |args: &[&str]| {
            let mut argv = vec!["probe", "clean"];
            argv.extend_from_slice(args);
            Probe::try_parse_from(argv)
        };
        assert!(parse(&["--dry-run", "--path", "/tmp/x"]).is_err(), "--dry-run with --path must be refused");
        assert!(parse(&["--dry-run", "--apply"]).is_err(), "--dry-run with --apply must be refused");
        assert!(parse(&["--path", "/tmp/x", "--apply"]).is_err(), "--path with --apply must be refused");

        let Ok(Probe { cmd: MaintCmd::ScratchGc { path, apply, .. } }) = parse(&["--path", "/tmp/x"]) else {
            panic!("--path alone must parse");
        };
        assert_eq!(path.as_deref(), Some(std::path::Path::new("/tmp/x")));
        assert!(!apply);
        assert!(parse(&["--apply"]).is_ok());
        assert!(parse(&["--dry-run"]).is_ok());
        assert!(parse(&[]).is_ok());
    }
}
