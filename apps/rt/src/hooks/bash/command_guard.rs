//! `command_guard` — a trava de comandos, no `PreToolUse` do Bash.
//!
//! O comando é lido uma vez, como o terminal o parte ([`lex`]), e passa por
//! quatro conferências, nesta ordem:
//!
//! - [`safety`] — recusa os comandos que destroem trabalho;
//! - [`reading`] — recusa leituras que expõem a chave; a recusa de um leitor
//!   simples traz pronto o comando que lê o arquivo com a chave escondida;
//! - [`windows_redirect`] — corrige, sem recusar, o redirecionamento para um
//!   caminho do Windows (`> C:\...`), que o shell POSIX transformaria num
//!   arquivo com nome estranho na pasta atual;
//! - [`waiting`] — recusa o laço que espera outro processo (`while`/`until`
//!   com `pgrep`, `pidof` ou `ps`) e corrige, sem recusar, a compilação ou o
//!   teste do `cargo` mandados para segundo plano ou chamados pelo caminho
//!   completo;
//!
//! [`reading`] recusa a leitura do `mustard.json` que guarda a chave do
//!   Jev: a recusa vence qualquer outro gancho que troque o comando e traz
//!   pronto o comando mascarado, ou uma orientação curta nos formatos sem
//!   troca segura. Ela também responde a busca por palavra
//!   em pastas de código com a marca do mapa: a resposta agrupada por função
//!   no lugar da busca que mostra linhas, quando o mapa crava; a busca comum
//!   com a nota do mapa junto, quando a marca é parcial (ela passa antes pelo
//!   filtro do mapa, pela mesma porta da busca por assunto); a busca comum com
//!   uma linha da marca quando ela só lista nomes ou conta, ou, sem achado, a
//!   busca comum com uma linha do que o mapa não achou.
//!
//! A primeira que decide vence. Depois delas, no shell do host e com a opção
//! `rtk` ligada, o comando passa pela troca do rtk ([`super::rtk_swap`]),
//! feita aqui mesmo com a cópia do rtk que acompanha o Mustard: o Mustard é o
//! único gancho que troca o comando do Bash.

use mustard_core::domain::model::contract::{Check, Ctx, HookInput, Trigger, Verdict};
use mustard_core::platform::error::Error;

use super::{lex, reading, safety, waiting, windows_redirect};

/// A trava de comandos do Bash.
pub struct CommandGuard;

impl CommandGuard {
    /// O texto do comando que o Bash vai rodar.
    fn command_of(input: &HookInput) -> Option<String> {
        input.tool_input.get("command").and_then(|v| v.as_str()).map(str::to_string)
    }

    /// As mesmas conferências para o comando que o próprio Mustard roda no
    /// processo dele, como a busca: sem shell do host, não há diretório do
    /// condutor a guardar, e o comando não volta embrulhado no subshell que
    /// devolve o condutor ao checkout principal.
    pub(crate) fn evaluate_in_process(input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        Self::verdict(input, ctx, false)
    }

    /// As conferências; `host_shell` diz se o comando roda no shell do host.
    fn verdict(input: &HookInput, ctx: &Ctx, host_shell: bool) -> Result<Verdict, Error> {
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
        if input.is_subagent()
            && segments.iter().any(|seg| {
                matches!(seg.name(), "mustard-rt" | "mustard-rt.exe" | "mustard-boot" | "mustard-boot.cmd")
                    && seg
                        .args
                        .windows(3)
                        .any(|args| args[0].text == "run" && args[1].text == "write" && matches!(args[2].text.as_str(), "task" | "request" | "wave"))
            })
        {
            return Ok(Verdict::Deny { reason: match ctx.config.language().text_or_default() {
                mustard_core::SupportedLocale::PtBr => "O agente não altera o plano diretamente. Relate sobras, obrigações e proposta no retorno da onda; o condutor e o runtime gravam a tarefa autorizada.".into(),
                mustard_core::SupportedLocale::EnUs => "An agent cannot edit the plan directly. Report leftovers, obligations and proposals in the wave return; the conductor and runtime write authorized tasks.".into(),
            }});
        }
        if let Some(verdict) = safety::bash_safety(&segments, &cmd, ctx) {
            return Ok(verdict);
        }
        // A cópia do rtk só é procurada quando a troca vai acontecer: shell do
        // host, projeto com mustard.json e a opção `rtk` ligada.
        let root = std::path::Path::new(&ctx.project_dir);
        let rtk = (host_shell && ctx.config.rtk() && mustard_core::ProjectConfig::exists(root))
            .then(super::rtk_swap::bundled)
            .flatten();
        let runner = mustard_core::platform::code_tools::MachineRunner::new(&std::env::var("PATH").unwrap_or_default()).at(root);
        let isolate = |verdict| super::workspace_scope::isolate(input, ctx, verdict, host_shell, rtk.as_deref().map(|copy| (copy, &runner)));
        let lang = ctx.config.language().text_or_default();
        if let Some(verdict) = reading::bash_reading(&segments, &cmd, input, ctx) {
            // A recusa vence a troca que outro gancho faça no mesmo comando; uma
            // troca feita aqui poderia perder para a dele. Quando o formato
            // permite, a recusa traz pronto, na última linha, o comando que lê
            // o arquivo com a chave escondida.
            let verdict = super::config_read::masked_command(&cmd, input, ctx).map_or(verdict, |command| Verdict::Deny {
                reason: crate::shared::say::say("config_key.masked_command", lang, &[("{command}", &command)]),
            });
            return Ok(isolate(verdict));
        }
        if let Some(verdict) = windows_redirect::bash_windows_redirect(&segments, &cmd, input, lang) {
            return Ok(isolate(verdict));
        }
        if let Some(verdict) = waiting::bash_waiting(&segments, &cmd, input, lang, rtk.as_deref()) {
            return Ok(isolate(verdict));
        }
        Ok(isolate(crate::shared::search_gateway::before_bash(input,ctx).unwrap_or(Verdict::Allow)))
    }
}

impl Check for CommandGuard {
    /// Roda as quatro conferências no `PreToolUse` do Bash; qualquer outro
    /// evento ou ferramenta passa.
    ///
    /// A trava dos comandos que destroem trabalho não tem modo: ela sempre
    /// recusa.
    fn evaluate(&self, input: &HookInput, ctx: &Ctx) -> Result<Verdict, Error> {
        Self::verdict(input, ctx, true)
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

    /// O redirecionamento para um caminho do Windows é reescrito, com uma
    /// nota do que trocou, para `>` e `2>` nos dois jeitos de escrever o
    /// caminho, e para o `tee`; o comando segue, sem recusa. Um comando que
    /// destrói trabalho, mesmo com um redirecionamento assim, continua
    /// recusado: a trava que sobra (`safety::bash_safety`) cobre o caso que
    /// a recusa desta trava cobria.
    #[test]
    fn a_windows_redirect_is_rewritten_instead_of_refused() {
        for (cmd, rewritten) in [
            ("cat src/main.rs > C:\\Atiz\\dump.txt", "cat src/main.rs > /c/Atiz/dump.txt"),
            ("rtk cargo test 2> D:/logs/erro.txt", "rtk cargo test 2> /d/logs/erro.txt"),
            ("cmd | tee C:\\Atiz\\out.txt", "cmd | tee /c/Atiz/out.txt"),
        ] {
            match verdict_for(cmd) {
                Verdict::Rewrite { tool_input, note } => {
                    assert_eq!(tool_input["command"], rewritten, "{cmd}");
                    assert!(note.is_some(), "{cmd}");
                }
                other => panic!("expected Rewrite for {cmd:?}, got {other:?}"),
            }
        }

        match verdict_for("rm -rvf /tmp/work > C:\\pasta\\arquivo.txt") {
            Verdict::Deny { .. } => {}
            other => {
                panic!("a destructive command stays denied even with a Windows redirect: {other:?}")
            }
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
        let input = HookInput { tool_name: Some("Write".to_string()), hook_event_name: Some("PreToolUse".to_string()), ..HookInput::default() };
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
