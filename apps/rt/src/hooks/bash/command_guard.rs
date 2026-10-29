//! `command_guard` — a trava de comandos, no `PreToolUse` do Bash.
//!
//! O comando é lido uma vez, como o terminal o parte ([`lex`]), e passa por
//! quatro conferências, nesta ordem:
//!
//! - [`base_delete`] — recusa apagar uma branch de integração do `git.flow`;
//!   os outros perigos fixos ficam no bloqueio nativo das configurações;
//! - [`runtime_output`] — recusa o comando que passa a resposta do Mustard
//!   por script, filtro ou corte (`| python3`, `| grep`, `| head`, `| cut`,
//!   `| sed`, `$(mustard-rt …)` entregue a um deles), na conversa principal e
//!   nos agentes: a resposta se lê inteira. Gravar num arquivo (`>`, `>>`),
//!   juntar os erros (`2>&1`) e dobrar as linhas (`| fold`) passam;
//! - [`shell_edit`] — recusa, só no agente de onda, o comando que mexe em
//!   arquivo do projeto pelo terminal (programa avulso, `sed -i`, texto
//!   redirecionado): ele lê o trecho e edita pela ferramenta de edição;
//! - [`reading`] — recusa leituras que expõem a chave; a recusa de um leitor
//!   simples traz pronto o comando que lê o arquivo com a chave escondida.
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

use super::{base_delete, lex, reading, runtime_output, shell_edit};

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
        if let Some(verdict) = base_delete::bash_base_delete(&segments, &cmd, ctx) {
            return Ok(verdict);
        }
        let lang = ctx.config.language().text_or_default();
        if let Some(verdict) = runtime_output::bash_runtime_output(&segments, &cmd, lang) {
            return Ok(verdict);
        }
        if let Some(verdict) = shell_edit::bash_shell_edit(&segments, input, ctx, lang) {
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
        if let Some(verdict) = reading::bash_reading(&segments, &cmd, input, ctx) {
            // A recusa vence a troca que outro gancho faça no mesmo comando; uma
            // troca feita aqui poderia perder para a dele. Quando o formato
            // permite, a recusa traz pronto, na última linha, o comando que lê
            // o arquivo com a chave escondida.
            let verdict = super::config_read::masked_command(&cmd, input, ctx).map_or(verdict, |command| Verdict::Deny {
                reason: crate::shared::say::say("config_key.masked_command", lang, &[("{command}", &command)]),
            });
            // A busca que varre o arquivo da chave também volta pronta: a
            // mesma ideia da leitura acima, com a opção que tira o arquivo
            // posta na etapa que varre (21/10/2026).
            let verdict = reading::excluded_command(&cmd, input, ctx).map_or(verdict, |command| Verdict::Deny {
                reason: crate::shared::say::say("config_key.swept_rewritten", lang, &[("{command}", &command)]),
            });
            return Ok(isolate(verdict));
        }
        Ok(isolate(crate::shared::search_gateway::before_bash(input,ctx).unwrap_or(Verdict::Allow)))
    }
}

impl Check for CommandGuard {
    /// Roda as conferências no `PreToolUse` do Bash; qualquer outro evento
    /// ou ferramenta passa.
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

    /// A recusa de apagar uma base vem primeiro e diz a branch; os perigos
    /// fixos ficam com o bloqueio nativo e passam por aqui.
    #[test]
    fn the_base_delete_refusal_comes_first_in_the_chain() {
        let (input, mut ctx) = pre_bash("git branch -D develop");
        ctx.config.git.flow.insert("*".to_string(), "develop".to_string());
        let reason = mustard_core::translate("command_guard.delete_base", SupportedLocale::PtBr).replace("{branch}", "develop");
        match CommandGuard.evaluate(&input, &ctx).expect("check never errors") {
            Verdict::Deny { reason: refused } => assert!(refused.contains(&reason), "reason: {refused}"),
            other => panic!("expected Deny, got {other:?}"),
        }
        for cmd in ["rm -rvf /tmp/work", "while pgrep -f x >/dev/null; do sleep 1; done", "cat src/main.rs > C:\\Atiz\\dump.txt"] {
            assert!(!verdict_for(cmd).is_blocking(), "{cmd}");
        }
    }

    /// Comando comum passa: ler ou buscar pelo terminal não é uma das
    /// conferências.
    #[test]
    fn ordinary_commands_pass_the_chain() {
        for cmd in ["git status", "npm run build", "grep -r pattern src/", "cat README.md", "git commit -m x"] {
            assert!(!verdict_for(cmd).is_blocking(), "{cmd}");
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
