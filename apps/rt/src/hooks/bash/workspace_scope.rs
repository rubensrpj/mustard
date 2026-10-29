//! O comando do Bash, depois das conferências: o teste do Jest com a raiz
//! escrita por inteiro, a troca do rtk, a identidade da sessão e do agente
//! para o comando do Mustard e a guarda da cópia da onda. A troca do rtk vem
//! antes das outras, que embrulham o comando que o rtk já encurtou.
use super::lex;
use mustard_core::{
    domain::model::contract::{Ctx, HookInput, Verdict},
    platform::code_tools::ToolRunner,
};
use std::path::Path;

/// `rtk` is the bundled rtk copy and the runner that asks it for the swap,
/// present only when the command runs in the host shell and the project keeps
/// the `rtk` option on.
pub(crate) fn isolate<R: ToolRunner>(input: &HookInput, ctx: &Ctx, verdict: Verdict, host_shell: bool, rtk: Option<(&Path, &R)>) -> Verdict {
    let verdict = normalize_test(input, ctx, verdict);
    let verdict = match rtk {
        Some((copy, runner)) if host_shell => super::rtk_swap::swap(input, verdict, copy, runner),
        _ => verdict,
    };
    let verdict = bind_identity(input, verdict);
    crate::hooks::write::wave_copy::guard_bash(input, ctx, verdict)
}

/// O comando nativo do Mustard fica sabendo quem o pediu: a sessão do host e,
/// dentro de um subagente, o agente. A busca confere o comando como esse agente.
fn bind_identity(input: &HookInput, verdict: Verdict) -> Verdict {
    let named = |id: &&str| !id.is_empty() && id.len() <= 160;
    let session = input.session_id.as_deref().filter(named).map(crate::shared::spec_state::shell_export);
    let agent = input.agent_id.as_deref().filter(named).map(crate::shared::search_gateway::agent_export);
    let prefix = session.unwrap_or_default() + &agent.unwrap_or_default();
    if prefix.is_empty() {
        return verdict;
    }
    let mut tool_input = match &verdict {
        Verdict::Allow => input.tool_input.clone(),
        Verdict::Rewrite { tool_input, .. } => tool_input.clone(),
        _ => return verdict,
    };
    let Some(command) = tool_input["command"].as_str() else {
        return verdict;
    };
    if !lex::segments(command).iter().any(lex::Segment::runs_mustard) {
        return verdict;
    }
    if command.starts_with(&prefix) {
        return verdict;
    }
    tool_input["command"] = serde_json::json!(format!("{prefix}{command}"));
    let note = match verdict {
        Verdict::Rewrite { note, .. } => note,
        _ => None,
    };
    Verdict::Rewrite { tool_input, note }
}

fn normalize_test(input: &HookInput, ctx: &Ctx, verdict: Verdict) -> Verdict {
    let (mut tool_input, previous_note) = match &verdict {
        Verdict::Allow => (input.tool_input.clone(), None),
        Verdict::Rewrite { tool_input, note } => (tool_input.clone(), note.clone()),
        _ => return verdict,
    };
    let Some(command) = tool_input["command"].as_str() else {
        return verdict;
    };
    let normalized = crate::commands::review::qa_run::canonical_test_command(command);
    if normalized == command {
        return verdict;
    }
    let note = match ctx.config.language().text_or_default() {
        mustard_core::SupportedLocale::PtBr => {
            "Jest: a raiz foi escrita como --roots=<pasta> para preservar o caminho do teste como argumento separado."
        }
        mustard_core::SupportedLocale::EnUs => "Jest: wrote the root as --roots=<folder> to preserve the test path as a separate argument.",
    };
    tool_input["command"] = serde_json::json!(normalized);
    Verdict::Rewrite { tool_input, note: Some(previous_note.map_or_else(|| note.to_string(), |previous| format!("{previous}\n{note}"))) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mustard_core::platform::code_tools::MachineRunner;
    use serde_json::json;
    const NO_RTK: Option<(&'static Path, &'static MachineRunner)> = None;
    #[test]
    fn native_cli_receives_the_exact_host_session_even_in_background() {
        let input: HookInput = serde_json::from_value(json!({"session_id":"current-session","tool_name":"Bash","tool_input":{"command":"mustard-rt run round --dispatch","run_in_background":true,"timeout":600_000}})).unwrap();
        let Verdict::Rewrite { tool_input, .. } = bind_identity(&input, Verdict::Allow) else {
            panic!("session propagation");
        };
        assert_eq!(tool_input["command"], "export MUSTARD_SESSION_ID='current-session'\nmustard-rt run round --dispatch");
        assert_eq!(tool_input["timeout"], 600_000);
        assert_eq!(tool_input["run_in_background"], true);
        assert!(matches!(bind_identity(&input, Verdict::Deny { reason: "protected".into() }), Verdict::Deny { .. }));
    }
    /// O teste do Jest sai com a raiz escrita por inteiro, e a recusa que
    /// já veio passa como veio.
    #[test]
    fn a_jest_root_is_normalized_and_an_existing_refusal_stays() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let ctx = Ctx::for_test(root.to_string_lossy().into_owned(), Some(mustard_core::domain::model::contract::Trigger::PreToolUse));
        let jest: HookInput =
            serde_json::from_value(json!({"cwd":root,"tool_name":"Bash","tool_input":{"command":"jest --roots src src/example.spec.ts"}})).unwrap();
        let Verdict::Rewrite { tool_input, .. } = isolate(&jest, &ctx, Verdict::Allow, true, NO_RTK) else {
            panic!("Jest root normalization");
        };
        assert_eq!(tool_input["command"], "jest --roots=src src/example.spec.ts");
        assert!(matches!(isolate(&jest, &ctx, Verdict::Deny { reason: "existing protection".into() }, true, NO_RTK), Verdict::Deny { .. }));
    }
}
