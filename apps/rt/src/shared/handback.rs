//! A devolução do relatório a quem conduz: o fim do trabalho de um agente.
//!
//! Mora aqui, e não dentro de uma trava, porque é a mesma regra para todas
//! elas. Nenhuma recusa a devolução: sem ela o relatório não chega a quem
//! conduz, e o agente termina sem entregar nada.
//!
//! Enquanto cada trava tinha a própria lista, a do agente de análise não
//! conhecia esta ferramenta, e barrou a devolução do recibo com a mensagem
//! que mandava devolver o recibo (campo, 21/10/2026).

use mustard_core::domain::model::contract::HookInput;

/// A ferramenta com que o agente devolve o relatório a quem conduz e termina
/// o trabalho.
pub(crate) const REPORT_BACK_TOOL: &str = "SubagentHandback";

/// Se a chamada `input` devolve o relatório a quem conduz
/// ([`REPORT_BACK_TOOL`]). Ela não grava na spec nem na cópia, e é o fim do
/// agente.
pub(crate) fn hands_back_the_report(input: &HookInput) -> bool {
    input.tool_name.as_deref() == Some(REPORT_BACK_TOOL)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As travas que perguntam aqui respondem igual à mesma chamada: é esse o
    /// ponto de a peça ser uma só.
    #[test]
    fn every_guard_sees_the_same_handback() {
        let call = |tool: &str| HookInput { tool_name: Some(tool.into()), ..HookInput::default() };
        assert!(hands_back_the_report(&call(REPORT_BACK_TOOL)));
        for other in ["Edit", "Write", "Bash", "Read"] {
            assert!(!hands_back_the_report(&call(other)), "{other}");
        }
        assert!(!hands_back_the_report(&HookInput::default()), "sem ferramenta, não é devolução");
    }
}
