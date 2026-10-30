//! The analysis role inherits native host permissions. Enforce its narrower
//! task at the supported PreToolUse boundary, not an ignored agent-file key.
use mustard_core::domain::model::contract::{HookInput, Trigger, Verdict};
use serde_json::Value;
use std::path::Path;

fn role(input: &HookInput) -> Option<&str> {
    input
        .agent_type
        .as_deref()
        .or_else(|| input.raw.get("agent_type").and_then(Value::as_str))
}
fn binding(root: &Path, input: &HookInput) -> Option<std::path::PathBuf> {
    let id = input.subagent_id()?;
    Some(root.join(".claude/.session").join(format!(
        "analysis-role-{}",
        blake3::hash(id.as_bytes()).to_hex()
    )))
}
fn analysis(root: &Path, input: &HookInput, trigger: Trigger) -> bool {
    let explicit = role(input).is_some_and(|role| {
        matches!(
            role.split(':').next_back(),
            Some("mustard-analysis" | "mustard_analysis")
        )
    });
    if trigger == Trigger::SubagentStart
        && explicit
        && let Some(path) = binding(root, input)
    {
        // No model or source data: only a role binding survives later events
        // where the host exposes the child id but omits its agent type.
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
            if parent
                .canonicalize()
                .ok()
                .zip(root.canonicalize().ok())
                .is_some_and(|(p, r)| p.starts_with(r))
            {
                let _ = mustard_core::io::fs::write_atomic(&path, b"analysis");
            }
        }
    }
    explicit
        || binding(root, input)
            .is_some_and(|path| std::fs::read(path).ok().as_deref() == Some(b"analysis"))
}
fn read_request(input: &Value) -> bool {
    let request = input.get("request").unwrap_or(input);
    matches!(request["tool"].as_str(), Some("Read" | "Grep" | "Glob"))
}
fn read_command(command: &str) -> bool {
    use crate::hooks::bash::lex;
    // Restrict the entire shell, not just the first executable. A quoted JSON
    // pattern can contain shell metacharacters; outer command ranges and raw
    // expansion checks keep that data distinct from executable substitutions.
    let segments = lex::segments(command);
    if lex::command_ranges(command).len() != 1 || segments.len() != 1 {
        return false;
    }
    let s = &segments[0];
    if !s.redirects.is_empty()
        || s.piped
        || !s.leading.is_empty()
        || !matches!(s.name(), "mustard-rt" | "mustard-rt.exe")
    {
        return false;
    }
    if s.program.raw != s.program.text || !command.trim_start().starts_with(&s.program.raw) {
        return false;
    }
    let args: Vec<_> = s.args.iter().map(|w| w.text.as_str()).collect();
    if args.len() < 2 || args[0] != "run" {
        return false;
    }
    // Shell expansions are never necessary in the bounded analysis contract.
    if s.args
        .iter()
        .any(|w| !w.raw.starts_with('\'') && w.raw.chars().any(|c| "$`\\\n".contains(c)))
    {
        return false;
    }
    let value_flags: &[&str] = match args[1] {
        "knowledge" => &[
            "--root",
            "--query",
            "--file",
            "--symbol",
            "--direction",
            "--limit",
            "--depth",
            "--intent",
            "--purpose",
        ],
        "search" => &["--root", "--request", "--intent", "--purpose"],
        "read" => &["--root", "--spec", "--term"],
        _ => return false,
    };
    let switches: &[&str] = match args[1] {
        "knowledge" => &[
            "--profile",
            "--coverage",
            "--refresh",
            "--all",
            "--detail",
            "--markdown",
        ],
        "search" => &["--shell-output", "--raw"],
        "read" => &["--all"],
        _ => &[],
    };
    let mut at = 2;
    while at < args.len() {
        let flag = args[at];
        if value_flags.contains(&flag) {
            let Some(value) = args.get(at + 1) else {
                return false;
            };
            if flag == "--request"
                && !serde_json::from_str::<Value>(value)
                    .is_ok_and(|v| read_request(v.get("request").unwrap_or(&v)))
            {
                return false;
            }
            at += 2;
        } else if switches.contains(&flag) || args[1] == "read" && at == 2 && !flag.starts_with('-')
        {
            at += 1;
        } else {
            return false;
        }
    }
    true
}

pub(crate) fn check(root: &Path, input: &HookInput, trigger: Trigger) -> Verdict {
    let requested = input.tool_input["subagent_type"]
        .as_str()
        .is_some_and(|name| name.split(':').next_back() == Some("mustard-analysis"));
    if trigger == Trigger::PreToolUse && input.raw["mustard_host"] == "codex" && requested {
        let resolution = mustard_core::platform::session_models::resolve("codex", None);
        let definition = mustard_core::platform::codex::analysis_configuration(root);
        // O esforço esperado vem do dono do par modelo e esforço, nunca de um
        // valor escrito aqui: a trava e o molde que ela confere têm de ler a
        // mesma coisa (regra de 21/10/2026).
        if resolution["ready"] != true
            || definition["model"] != resolution["model"]
            || definition["effort"] != resolution["effort"]
        {
            // O esforço no texto vem do dono, nunca escrito aqui: a mensagem
            // que crava o valor passa a mentir quando o dono muda. E o texto
            // sai do catálogo, na língua do projeto, como as outras recusas.
            let effort = mustard_core::platform::session_models::effort("codex");
            let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
            return Verdict::Deny {
                reason: crate::shared::say::say("analysis_guard.codex_unavailable", lang, &[("{effort}", effort)]),
            };
        }
    }
    if !analysis(root, input, trigger) || trigger != Trigger::PreToolUse {
        return Verdict::Allow;
    }
    if crate::hooks::session::conversation_size::analysis_context(input)
        .is_some_and(|n| n >= 90_000)
    {
        return Verdict::Deny {reason:"[Mustard] Analysis is near its fixed 100000-token input budget; keep headroom for the final response. Return the source-bound result now; do not read more or start another task. Unknown usage is never treated as verified compliance.".into()};
    }
    // O esforço esperado sai do dono do par modelo e esforço, na comparação e
    // no texto: um valor escrito aqui passa a mentir quando o dono muda.
    let codex_effort = mustard_core::platform::session_models::effort("codex");
    if input.raw["mustard_host"] == "codex"
        && (input.raw["model"]
            .as_str()
            .is_some_and(|model| !model.split('-').any(|part| part == "luna"))
            || input.raw["effort"]
                .as_str()
                .is_some_and(|effort| effort != codex_effort))
    {
        let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
        return Verdict::Deny {
            reason: crate::shared::say::say("analysis_guard.codex_family", lang, &[("{effort}", codex_effort)]),
        };
    }
    // A devolução do relatório a quem conduz nunca é barrada, pela peça que
    // todas as travas usam: a mensagem abaixo manda devolver o recibo, e
    // enquanto esta lista não conhecia a ferramenta da devolução era ela mesma
    // que o impedia (campo, 21/10/2026).
    if crate::shared::handback::hands_back_the_report(input) {
        return Verdict::Allow;
    }
    let allowed = match input.tool_name.as_deref() {
        Some("Read" | "Grep" | "Glob" | "ToolSearch") => true,
        Some("mcp__mustard__search") => read_request(&input.tool_input),
        Some("Bash") => input.tool_input["command"]
            .as_str()
            .is_some_and(read_command),
        _ => false,
    };
    if allowed {
        Verdict::Allow
    } else {
        let lang = mustard_core::ProjectConfig::load(root).language().text_or_default();
        let tool = input.tool_name.as_deref().unwrap_or("-");
        Verdict::Deny { reason: crate::shared::say::say("analysis_guard.read_only", lang, &[("{tool}", tool)]) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn literal_reads_work_but_mutations_and_shell_bypasses_do_not() {
        for command in [
            "mustard-rt run knowledge --profile",
            "mustard-rt run knowledge --query 'save $record' --file store.rs",
            "mustard-rt run read item-MSTD-RULE-0001 --root '/project path'",
            "mustard-rt run search --request '{\"tool\":\"Read\",\"input\":{\"file_path\":\"store.rs\"}}' --shell-output",
        ] {
            assert!(read_command(command), "{command}");
        }
        for command in [
            "mustard-rt run knowledge --out /tmp/result.json",
            "mustard-rt run knowledge --record receipt.json",
            "mustard-rt run read x; touch other",
            "mustard-rt run knowledge --query $(touch other)",
            "mustard-rt run knowledge --query x > other",
            "env X=y mustard-rt run knowledge --profile",
            "mustard-rt run search --request '{\"tool\":\"Bash\",\"input\":{\"command\":\"touch other\"}}'",
        ] {
            assert!(!read_command(command), "{command}");
        }
    }
    /// A devolução do recibo a quem conduz passa, e a recusa das outras
    /// ferramentas sai na língua do projeto, nomeando a ferramenta barrada.
    ///
    /// Em campo (21/10/2026) a devolução do agente de análise foi barrada duas
    /// vezes, com a mensagem que mandava devolver o recibo: a lista de
    /// permitidas não conhecia a ferramenta da devolução. A trava do agente de
    /// onda já a conhecia, e é dela que esta agora pergunta.
    #[test]
    fn handing_the_receipt_back_passes_and_the_refusal_speaks_the_project_language() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let analysis = |tool: &str| HookInput {
            tool_name: Some(tool.into()),
            raw: json!({"agent_id": "child", "agent_type": "mustard-analysis"}),
            agent_type: Some("mustard-analysis".into()),
            ..Default::default()
        };
        assert_eq!(
            check(root, &analysis("SubagentHandback"), Trigger::PreToolUse),
            Verdict::Allow,
            "a devolução do recibo nunca é barrada",
        );
        let Verdict::Deny { reason } = check(root, &analysis("Edit"), Trigger::PreToolUse) else {
            panic!("editar continua barrado para o agente de análise");
        };
        assert!(reason.contains("Edit"), "a recusa nomeia a ferramenta barrada: {reason}");
        assert!(reason.contains("só lê"), "a recusa sai em português, a língua padrão: {reason}");
    }

    #[test]
    fn native_child_role_binding_guards_later_tools_without_affecting_parent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut child = HookInput {
            agent_type: Some("mustard-analysis".into()),
            raw: json!({"agent_id":"child"}),
            ..Default::default()
        };
        assert_eq!(check(root, &child, Trigger::SubagentStart), Verdict::Allow);
        child.agent_type = None;
        child.tool_name = Some("Edit".into());
        assert!(matches!(
            check(root, &child, Trigger::PreToolUse),
            Verdict::Deny { .. }
        ));
        child.raw = json!({"agent_id":"parent"});
        assert_eq!(check(root, &child, Trigger::PreToolUse), Verdict::Allow);
    }
}
