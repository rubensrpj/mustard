//! As marcas que os ganchos do despacho leem.
//!
//! O comando que montava o pedido do agente saiu: quem monta hoje é a rodada
//! do fluxo. Ficaram só o piso epistêmico injetado em toda chamada e a escolha
//! do tipo de subagente, porque os ganchos de contexto e de injeção os leem em
//! cada despacho.

pub use super::render::PROMPT_REF_MARKER;

/// A disciplina de prova que todo texto de agente carrega: enumerar antes de
/// afirmar, e nunca dizer que algo não existe a partir de uma leitura por
/// amostra.
pub const EPISTEMIC_FLOOR: &str = "Settle existence/duplication questions by Grep \
     enumeration over the slice FIRST — reading samples never proves absence. Ground \
     every claim in file:line. NEVER assert \"X does not exist\" and never refute a \
     symptom the user observed at runtime — static reading cannot disprove it; say \
     \"not found in the files I read\" instead.";

/// O prefixo do nome de um agente que vem do plugin.
const PLUGIN_NAMESPACE: &str = "mustard";

/// O tipo de subagente que um papel pede. Os nomes dos agentes do plugin vêm
/// prefixados; os embutidos do Claude Code, não.
#[must_use]
pub fn recommended_subagent_type(role: &str) -> String {
    match role.trim().to_ascii_lowercase().as_str() {
        "explore" => "Explore".to_string(),
        "plan" => "Plan".to_string(),
        "review" | "qa" => format!("{PLUGIN_NAMESPACE}:review"),
        _ => "general-purpose".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::recommended_subagent_type;

    /// O revisor vem do plugin e leva o prefixo; os embutidos, não.
    #[test]
    fn o_revisor_leva_o_prefixo_do_plugin_e_os_embutidos_nao() {
        assert_eq!(recommended_subagent_type("review"), "mustard:review");
        assert_eq!(recommended_subagent_type("qa"), "mustard:review");
        assert_eq!(recommended_subagent_type("explore"), "Explore");
        assert_eq!(recommended_subagent_type("plan"), "Plan");
        assert_eq!(recommended_subagent_type("impl"), "general-purpose");
    }
}
