//! A linha de comando de `mustard-rt`: o programa e as suas duas portas.
//!
//! Mora fora do `main.rs` porque o binário não roda teste (`test = false` em
//! `apps/rt/Cargo.toml`). A biblioteca declara este módulo também, e assim o
//! teste da ajuda percorre a árvore que a pessoa digita, a partir da raiz: o
//! texto do programa, o de `on` e o de cada comando de `run`.

use crate::commands;
use clap::{Parser, Subcommand};

/// The `mustard-rt` command line.
#[derive(Debug, Parser)]
#[command(name = "mustard-rt", version = env!("MUSTARD_VERSION_FULL"), about = "Mustard enforcement runtime")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// As faces do binário. `Run` não lê o stdin.
#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI parser enum — single-use stack alloc, indirection adds no value
pub enum Command {
    /// Roda os ganchos de um evento do Claude Code.
    On {
        /// O nome do evento, como `PreToolUse` ou `Stop`.
        event: String,
        /// Host protocol; Codex normalizes tools and has distinct lifecycle output.
        #[arg(long, default_value = "claude", value_parser = ["claude", "codex"])]
        host: String,
    },
    /// Native stdio search server for the host's existing session.
    Mcp {
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
    },
    /// Roda um comando. Recebe argumentos, não o stdin.
    Run {
        #[command(subcommand)]
        command: commands::RunCmd,
    },
}

/// O argumento que o terminal entregou inteiro, quando foi isso que
/// aconteceu: o valor começa por `--` e traz outro `--` adiante, e um valor só
/// nunca tem essa forma. É o que sai de `mustard-rt run read $SPEC`, com a
/// variável guardando `--root /projeto --spec obra`: o terminal desta máquina
/// entrega a variável como um argumento só.
///
/// `None` para qualquer outro valor. O caminho com espaço é um valor legítimo,
/// e ele não começa por `--`.
fn glued_argument(error: &clap::Error) -> Option<String> {
    let found = error.get(clap::error::ContextKind::InvalidArg).or_else(|| error.get(clap::error::ContextKind::InvalidValue))?;
    let whole = found.to_string().trim().to_string();
    let mut pieces = whole.split_whitespace();
    let first = pieces.next()?;
    (first.starts_with("--") && pieces.any(|piece| piece.starts_with("--"))).then_some(whole)
}

/// Machine-readable failures for the command face; help/version retain
/// clap's ordinary text and exit status.
///
/// O argumento que o terminal entregou inteiro ganha uma recusa própria, em
/// português, com a forma certa: a do clap diz, em inglês, que existe um
/// argumento parecido, e manda procurar no lugar errado. Foram quarenta
/// recusas assim num projeto de verdade.
pub(crate) fn argument_failure(error: &clap::Error) -> serde_json::Value {
    let hint = error.to_string();
    if let Some(glued) = glued_argument(error) {
        // O idioma é o do projeto de onde o comando saiu, como em toda recusa
        // do Mustard: a do clap sai sempre em inglês.
        let lang = crate::commands::spec_events::project(std::path::Path::new(".")).lang;
        let said = crate::shared::say::say("command_guard.glued_argument", lang, &[("{argument}", &glued)]);
        return serde_json::json!({"ok":false,"reason":"glued-arguments","kind":format!("{:?}", error.kind()),"hint":said});
    }
    let reason = if hint.contains("cannot be used multiple times") { "duplicate-option" } else { "invalid-arguments" };
    serde_json::json!({"ok":false,"reason":reason,"kind":format!("{:?}", error.kind()),"hint":hint})
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// O argumento que o terminal entregou inteiro ganha a recusa própria, em
    /// português, com a forma certa, e não a do clap, que diz em inglês que
    /// existe um argumento parecido. O caminho com espaço, que é um valor
    /// legítimo, segue com a recusa do clap.
    ///
    /// Em campo (21/10/2026) foram quarenta recusas assim: os agentes guardam
    /// a pasta e o nome da spec numa variável, e o shell desta máquina entrega
    /// a variável como um argumento só.
    #[test]
    fn a_variable_the_shell_handed_over_whole_gets_its_own_refusal() {
        let failing = |argv: &[&str]| {
            let error = Cli::try_parse_from(argv).expect_err("a linha é recusada");
            argument_failure(&error)
        };

        let glued = failing(&["mustard-rt", "run", "read", "backlog", "--root /projeto --spec obra"]);
        assert_eq!(glued["reason"], serde_json::json!("glued-arguments"), "{glued}");
        let said = glued["hint"].as_str().unwrap_or_default();
        assert!(said.contains("--root /projeto --spec obra"), "a recusa mostra o que chegou: {said}");
        assert!(said.contains("variável"), "e diz o que aconteceu, em português: {said}");
        assert!(!said.contains("similar"), "a sugestão do clap não aparece: {said}");

        // Um valor legítimo com espaço não é variável colada.
        let path = failing(&["mustard-rt", "run", "read", "backlog", "--root", "/pasta com espaco", "--nao-existe"]);
        assert_eq!(path["reason"], serde_json::json!("invalid-arguments"), "{path}");
        // A opção repetida segue com a recusa dela.
        let twice = failing(&["mustard-rt", "run", "read", "backlog", "--spec", "a", "--spec", "b"]);
        assert!(twice["reason"] != serde_json::json!("glued-arguments"), "{twice}");
    }
}
