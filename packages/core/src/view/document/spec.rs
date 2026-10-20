//! O que a página de uma spec carrega: o estado de uma onda e um dia da
//! economia do rtk.
//!
//! O motor que montava a página inteira da spec e a do projeto a partir do
//! arquivo de eventos, item por item, saiu com o comando que só ele servia:
//! a página de uma spec e a do projeto só existem hoje como template mais
//! banco de dados.

use std::collections::BTreeMap;

/// Um dia da economia do rtk neste projeto, como o próprio rtk a conta.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RtkDay {
    /// O dia, `2026-09-11`.
    pub date: String,
    /// Quantos comandos passaram pelo rtk nesse dia.
    pub commands: u64,
    /// Os tokens que a saída dos comandos teria sem o rtk.
    pub input: u64,
    /// Os tokens que o rtk tirou da saída.
    pub saved: u64,
}

/// Em que pé está uma onda, pela leitura que decide o que a rodada despacha.
/// Nenhuma onda fica como `Delivered` hoje: a rodada não pede revisão de onda
/// nenhuma, e a entrega já vale como aprovada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveState {
    /// Nada saiu ainda, ou o que saiu não vale mais.
    Todo,
    /// O pedido saiu, o agente começou e a entrega ainda não voltou.
    Running,
    /// O pedido saiu e nenhum agente trabalha nele: o agente nunca começou,
    /// ou o Claude Code que mandou o pedido já fechou.
    NoAgent,
    /// Entregue e aprovada, sem commit próprio: a entrega entrou na obra, como
    /// um relatório, sem mudar código.
    Approved,
    /// Entregue, aprovada e com o commit dela gravado.
    Committed,
    /// A última revisão reprovou.
    Rejected,
}

impl WaveState {
    /// A onda terminou: entregue e aprovada, com commit ou sem.
    #[must_use]
    pub fn finished(self) -> bool {
        matches!(self, Self::Approved | Self::Committed)
    }

    /// O nome do estado nas respostas do binário.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Running => "running",
            Self::NoAgent => "no-agent",
            Self::Approved => "integrated",
            Self::Committed => "committed",
            Self::Rejected => "rejected",
        }
    }
}

/// O estado de cada onda do plano, pelo número dela: toda onda do plano está
/// aqui, a por fazer inclusive. A onda que não está aqui saiu do plano, e a
/// página não a lista.
pub type WaveStates = BTreeMap<u64, WaveState>;

#[cfg(test)]
mod tests {
    use crate::domain::text::{code_lines, CODE_LINE_CAP};

    /// Este arquivo não tem mais partes: o motor que montava a página inteira
    /// da spec saiu, e o que ficou não passa do teto de linhas de código, pela
    /// medida única do núcleo.
    #[test]
    fn the_file_does_not_go_over_the_code_line_cap() {
        let gate = crate::manifest_dir::manifest_dir().join("src").join("view").join("document").join("spec.rs");
        let source = std::fs::read_to_string(&gate).unwrap_or_default();
        assert!(code_lines(&source) <= CODE_LINE_CAP, "{}", gate.display());
    }
}
