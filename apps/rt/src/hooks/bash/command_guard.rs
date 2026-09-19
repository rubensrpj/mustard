//! `command_guard` — a trava de comandos, no `PreToolUse` do Bash.
//!
//! O comando é lido uma vez, como o terminal o parte ([`lex`]), e passa por
//! três conferências, nesta ordem:
//!
//! - [`safety`] — recusa os comandos que destroem trabalho;
//! - [`windows_redirect`] — recusa o redirecionamento para um caminho do
//!   Windows (`> C:\...`), que o shell POSIX transformaria num arquivo com
//!   nome estranho na pasta atual;
//! - [`waiting`] — recusa o laço que espera outro processo (`while`/`until`
//!   com `pgrep`, `pidof` ou `ps`) e corrige, sem recusar, a compilação ou o
//!   teste do `cargo` mandados para segundo plano ou chamados pelo caminho
//!   completo.
//!
//! A primeira que decide vence. Trocar o `cargo` da linha de comando por
//! `rtk` não é feito aqui: o gancho do próprio rtk faz isso; `waiting` só
//! cobre o caminho completo, que esse gancho não alcança.

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;

use super::{lex, safety, waiting, windows_redirect};

/// A trava de comandos do Bash.
pub struct CommandGuard;

impl CommandGuard {
    /// O texto do comando que o Bash vai rodar.
    fn command_of(input: &HookInput) -> Option<String> {
        input.tool_input.get("command").and_then(|v| v.as_str()).map(str::to_string)
    }
}

impl Check for CommandGuard {
    /// Roda as duas conferências no `PreToolUse` do Bash; qualquer outro
    /// evento ou ferramenta passa.
    ///
    /// A trava dos comandos que destroem trabalho não tem modo: ela sempre
    /// recusa.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        if ctx.trigger != Some(Trigger::PreToolUse) {
            return Ok(Verdict::Allow);
        }
        if input.tool_name.as_deref() != Some("Bash") {
            return Ok(Verdict::Allow);
        }
        let Some(cmd) = Self::command_of(input) else {
            return Ok(Verdict::Allow);
        };
        let segments = lex::segments(&cmd);
        if let Some(verdict) = safety::bash_safety(&segments, &cmd, ctx) {
            return Ok(verdict);
        }
        let lang = ctx.config.language().text_or_default();
        if let Some(verdict) = windows_redirect::bash_windows_redirect(&segments, &cmd, lang) {
            return Ok(verdict);
        }
        if let Some(verdict) = waiting::bash_waiting(&segments, &cmd, input, lang) {
            return Ok(verdict);
        }
        Ok(Verdict::Allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::SupportedLocale;
    use serde_json::json;

    fn pre_bash(command: &str) -> (HookInput, Ctx) {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": command }),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PreToolUse));
        (input, ctx)
    }

    /// O veredito da trava para um comando do Bash.
    fn verdict_for(command: &str) -> Verdict {
        let (input, ctx) = pre_bash(command);
        CommandGuard.evaluate(&input, &ctx).expect("check never errors")
    }

    /// A recusa de um comando que destrói trabalho vem primeiro e diz o
    /// perigo que achou.
    #[test]
    fn the_command_guard_refusal_comes_first_in_the_chain() {
        let danger = mustard_core::translate("command_guard.rm_recursive_force", SupportedLocale::PtBr);
        match verdict_for("rm -rvf /tmp/work") {
            Verdict::Deny { reason } => assert!(reason.contains(danger), "reason: {reason}"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    /// O envio forçado escrito depois da branch continua recusado; a forma
    /// segura, com `--force-with-lease`, passa.
    #[test]
    fn force_push_denied_lease_allowed_through_chain() {
        assert!(verdict_for("git push origin dev --force").is_blocking());
        assert!(!verdict_for("git push --force-with-lease origin dev").is_blocking());
    }

    /// O redirecionamento para um caminho do Windows é recusado com o motivo
    /// próprio dele.
    #[test]
    fn a_windows_path_redirect_is_refused_with_its_own_reason() {
        let cmd = "cat src/main.rs > C:\\Atiz\\dump.txt";
        let expected = mustard_core::translate("command_guard.windows_path", SupportedLocale::PtBr)
            .replace("{target}", "C:\\Atiz\\dump.txt")
            .replace("{command}", cmd);
        match verdict_for(cmd) {
            Verdict::Deny { reason } => assert_eq!(reason, expected),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    /// Comando comum passa: a trava tem só as três conferências, e ler ou
    /// buscar pelo terminal não é uma delas.
    #[test]
    fn ordinary_commands_pass_the_chain() {
        for cmd in ["git status", "npm run build", "grep -r pattern src/", "cat README.md", "git commit -m x"] {
            assert!(!verdict_for(cmd).is_blocking(), "{cmd}");
        }
    }

    /// O laço que espera outro processo é recusado pela trava inteira, o
    /// mesmo veredito que a conferência sozinha (`waiting::bash_waiting`) dá;
    /// e o `cargo` mandado para segundo plano, que não é recusado, chega
    /// reescrito com o campo `run_in_background` fora e o teto de tempo.
    #[test]
    fn the_waiting_check_runs_inside_the_full_chain() {
        assert!(verdict_for("while pgrep -f x >/dev/null; do sleep 1; done").is_blocking());

        let (mut input, ctx) = pre_bash("cargo test -p mustard-rt");
        input.tool_input["run_in_background"] = json!(true);
        match CommandGuard.evaluate(&input, &ctx).expect("check never errors") {
            Verdict::Rewrite { tool_input, .. } => {
                assert_eq!(tool_input["command"], "cargo test -p mustard-rt");
                assert_eq!(tool_input["timeout"], 600_000);
                assert!(tool_input.get("run_in_background").is_none(), "{tool_input}");
            }
            other => panic!("expected a rewrite, got {other:?}"),
        }
    }

    #[test]
    fn non_bash_tool_allows() {
        let input = HookInput {
            tool_name: Some("Write".to_string()),
            hook_event_name: Some("PreToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PreToolUse));
        assert_eq!(CommandGuard.evaluate(&input, &ctx).expect("no error"), Verdict::Allow);
    }

    /// A trava só roda no `PreToolUse`: em qualquer outro evento, passa.
    #[test]
    fn non_pre_tool_use_trigger_allows() {
        let input = HookInput {
            tool_name: Some("Bash".to_string()),
            tool_input: json!({ "command": "rm -rf /" }),
            hook_event_name: Some("PostToolUse".to_string()),
            ..HookInput::default()
        };
        let ctx = Ctx::for_test(String::new(), Some(Trigger::PostToolUse));
        assert_eq!(CommandGuard.evaluate(&input, &ctx).expect("no error"), Verdict::Allow);
    }
}
