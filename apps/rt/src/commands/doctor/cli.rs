//! The `run` subcommands for health checks and audits (`doctor/`).
//!
//! FOUR registrations per command. Two live in this file: the variant in
//! [`DoctorCmd`] AND its arm in [`dispatch`] below; forgetting the arm still
//! compiles, but the command vanishes from the CLI. The other two live in
//! the tests: the name in `tests/run_command_surface.rs`, and a caller (or a
//! justified `RUNTIME_WHITELIST` line) in `tests/template_parity.rs`.
//!
//! [`crate::commands::RunCmd`] hoists this enum with `#[command(flatten)]`, so
//! every name stays FLAT: `mustard-rt run <name>`, never `run doctor <name>`.
//! `display_order` pins each command to its historical slot in the flat
//! `run --help` listing (clap sorts subcommands by `(display_order, name)`) -
//! splitting the god-enum into families must not reshuffle the published CLI.

use clap::Subcommand;

use crate::commands::{doctor};

/// The `run` subcommands owned by health checks and audits (`doctor/`).
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum - clap-Subcommand; boxing breaks derive
pub enum DoctorCmd {
    /// Read-only installation health diagnostic: wiring, drift, state health,
    /// wave-integrity and (optionally) residue. Prints a compact OK/WARN/FAIL
    /// report and exits 1 if any category is FAIL, 0 otherwise.
    ///
    /// Pass `--json` as a shortcut for `--format json`.
    #[command(display_order = 20)]
    Doctor {
        /// Also scan for dead file/script references (slower).
        #[arg(long)]
        residue: bool,
        /// Roda uma conferência sozinha. A lista é fechada: o parser recusa
        /// um nome que não esteja nela, em vez de o comando responder um
        /// relatório vazio que se lê como "está tudo certo".
        #[arg(long, value_parser = [
            "wave-integrity",
            "branch-protection",
            "spec-index",
            "scan-output",
        ])]
        check: Option<String>,
        /// O formato da saída. Lista fechada: `text` (padrão) ou `json`.
        #[arg(long, default_value = "text", value_parser = ["text", "json"])]
        format: String,
        /// Shorthand for `--format json`.
        #[arg(long)]
        json: bool,
    },
}

/// Dispatch one `doctor`-family `run` subcommand.
pub fn dispatch(cmd: DoctorCmd) {
    match cmd {
        DoctorCmd::Doctor { residue, check, format, json } => {
            // `--json` is a shorthand for `--format json`.
            let effective_format = if json { "json".to_string() } else { format };
            doctor::doctor::run(doctor::doctor::DoctorOpts {
                residue,
                check,
                format: effective_format,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    /// O comando montado só para exercitar o parser do diagnóstico.
    #[derive(Debug, Parser)]
    struct Harness {
        #[command(subcommand)]
        cmd: super::DoctorCmd,
    }

    /// As três conferências que auditavam o `.claude/` — o catálogo de pastas,
    /// o `.claude/` aninhado com estado e a sequência `.claude/.claude/` — não
    /// são conferências que o contrato nomeia, e saíram.
    ///
    /// O defeito que este teste pega é o nome voltar para a lista do `--check`
    /// sem que nada mais volte: o parser aceita, o diagnóstico não tem o que
    /// rodar, e a pessoa lê um relatório vazio como "está tudo certo".
    #[test]
    fn o_diagnostico_recusa_conferencia_que_o_contrato_nao_nomeia() {
        for nome in ["claude-paths", "workspace-leaks", "i1"] {
            assert!(
                Harness::try_parse_from(["x", "doctor", "--check", nome]).is_err(),
                "--check {nome} tem de ser recusado: não é conferência do contrato",
            );
        }
    }

    /// E o que o contrato nomeia continua respondendo, para o teste acima não
    /// passar por um parser que recusa tudo.
    #[test]
    fn as_conferencias_do_contrato_continuam_aceitas() {
        for nome in ["branch-protection", "spec-index"] {
            assert!(
                Harness::try_parse_from(["x", "doctor", "--check", nome]).is_ok(),
                "--check {nome} é do contrato e tem de ser aceito",
            );
        }
    }

    /// A ajuda não pode prometer uma conferência que saiu: a pessoa lê o nome
    /// ali e o digita.
    #[test]
    fn a_ajuda_do_diagnostico_nao_promete_conferencia_que_saiu() {
        let mut arvore = Harness::command();
        let diagnostico = arvore
            .find_subcommand_mut("doctor")
            .expect("o diagnóstico tem de estar registrado");
        let ajuda = diagnostico.render_long_help().to_string();
        for nome in ["claude-paths", "workspace-leaks"] {
            assert!(
                !ajuda.contains(nome),
                "a ajuda ainda promete a conferência '{nome}', que saiu",
            );
        }
    }
}
