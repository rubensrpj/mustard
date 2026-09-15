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
    /// Check (or apply) freshness of managed artifacts against their upstreams.
    ///
    /// Maintainer-side: reads `apps/cli/templates/.artifacts.json` and probes
    /// each external upstream. Fail-open — network errors degrade an artifact
    /// to `unknown` and never fail the command.
    #[command(display_order = 41)]
    ArtifactUpdate {
        /// Probe upstreams and emit the JSON freshness report (the default).
        #[arg(long)]
        check: bool,
        /// Pull updates into vendored trees / bump pinned versions.
        #[arg(long)]
        apply: bool,
        /// Manifest path (default `apps/cli/templates/.artifacts.json`).
        #[arg(long)]
        manifest: Option<String>,
    },
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
    #[command(name = "scratch-gc")]
    #[command(display_order = 88)]
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
    /// Kill-switch: set `"disableAllHooks": true` in `.claude/settings.json`
    /// and wipe volatile harness state (`.agent-state/`,
    /// `.cluster-cache.json`). Everything else in the file —
    /// `permissions.allow`/`deny`, `statusLine`, `env` — is preserved, and so
    /// are worktrees: `.claude/worktrees/` holds uncommitted work and is only
    /// never removed by silencing the harness.
    /// Restore with [`Self::Rehook`].
    ///
    /// `--scope this` (default) acts on the current repo's `.claude/` only.
    /// `--scope monorepo` also sweeps every `apps/*/.claude/` +
    /// `packages/*/.claude/`. `--scope all` adds the user-global
    /// `~/.claude/settings.json`, gated by `--confirm` (otherwise reported as
    /// `state: "skipped"`). Emits a pretty JSON report.
    #[command(display_order = 47)]
    Unhook {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Scope: `this` (default), `monorepo`, or `all`.
        #[arg(long, default_value = "this")]
        scope: String,
        /// Required for `--scope all` to also touch the user-global
        /// `~/.claude/settings.json`.
        #[arg(long)]
        confirm: bool,
    },
    /// Reverse [`Self::Unhook`]: in each `.claude/` in scope, remove
    /// `"disableAllHooks"` from `settings.json` — or, for a project unhooked
    /// by an older build, rename the newest `settings.json.disabled*` snapshot
    /// back. Volatile state directories that `unhook` wiped are left alone —
    /// the runtime regenerates them on the next run. Emits a pretty JSON report.
    #[command(display_order = 48)]
    Rehook {
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long, default_value = "this")]
        scope: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Audit (and optionally remove) drift in a project's `.claude/` directory.
    ///
    /// Enumerates every direct child of `.claude/`, classifies each against a
    /// declared consumer list (KEEP / STALE / ORPHAN / LEGACY / CACHE), and
    /// either reports candidates (default `--dry-run`) or removes the ORPHAN
    /// / LEGACY ones (`--apply`). Emits byte-stable pretty JSON; fail-open at
    /// every step — exit code is always 0.
    #[command(name = "claude-dir-prune")]
    #[command(display_order = 56)]
    ClaudeDirPrune {
        /// Repo root override. Defaults to the current working directory.
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Preview only — emit the report, mutate nothing (the default).
        #[arg(long, default_value_t = true, conflicts_with = "apply")]
        dry_run: bool,
        /// Apply the removals. Required to mutate the filesystem.
        #[arg(long)]
        apply: bool,
        /// Reserved for parity with sibling subcommands — JSON is the only
        /// format today, but the flag exists so callers can pass it.
        #[arg(long)]
        json: bool,
    },
    /// Install dependencies in every detected subproject.
    #[command(name = "maint-deps")]
    #[command(display_order = 60)]
    MaintDeps {
        /// Preview only — print the resolved install commands without running.
        #[arg(long)]
        dry_run: bool,
    },
    /// Run build/type-check validation in every detected subproject.
    #[command(name = "maint-validate")]
    #[command(display_order = 61)]
    MaintValidate {
        /// Preview only — print the resolved validate commands without running.
        #[arg(long)]
        dry_run: bool,
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
    #[command(display_order = 37)]
    Upsert {},
}

/// Dispatch one `maint`-family `run` subcommand.
pub fn dispatch(cmd: MaintCmd) {
    match cmd {
        MaintCmd::ArtifactUpdate {
            check,
            apply,
            manifest,
        } => maint::artifact_update::run(check, apply, manifest.as_deref()),
        MaintCmd::ScratchGc { dry_run, apply, path } => {
            // `dry_run` vale `true` por padrão e o `conflicts_with_all` recusa
            // `--dry-run` junto de `--apply` OU de `--path`: quando um dos dois
            // chega aqui, `dry_run` é só o padrão, nunca um pedido explícito.
            // Por isso descartá-lo é seguro — quem decide é `--apply`/`--path`.
            let _ = dry_run;
            maint::scratch_gc::run(maint::scratch_gc::ScratchGcOpts { apply, path });
        }
        MaintCmd::Unhook { repo, scope, confirm } => {
            maint::unhook::run(maint::unhook::UnhookOpts { repo, scope, confirm });
        }
        MaintCmd::Rehook { repo, scope, confirm } => {
            maint::rehook::run(maint::rehook::RehookOpts { repo, scope, confirm });
        }
        MaintCmd::ClaudeDirPrune {
            repo,
            dry_run,
            apply,
            json,
        } => {
            // `dry_run` defaults to `true`; clap's `conflicts_with` blocks
            // both flags from coexisting. `--apply` is the authoritative
            // mutator flag.
            let _ = dry_run;
            maint::claude_dir_prune::run(maint::claude_dir_prune::ClaudeDirPruneOpts {
                repo,
                apply,
                json,
            });
        }
        MaintCmd::MaintDeps { dry_run } => {
            maint::maint_deps::run(maint::maint_deps::MaintDepsOpts { dry_run });
        }
        MaintCmd::MaintValidate { dry_run } => {
            maint::maint_validate::run(maint::maint_validate::MaintValidateOpts { dry_run });
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
            let mut argv = vec!["probe", "scratch-gc"];
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
