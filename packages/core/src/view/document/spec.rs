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
    /// O pedido saiu e a entrega ainda não voltou.
    Running,
    /// Entregue e aprovada.
    Approved,
    /// A última revisão reprovou.
    Rejected,
}

/// O estado de cada onda, pelo número dela. A onda que não está aqui está
/// por fazer.
pub type WaveStates = BTreeMap<u64, WaveState>;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::domain::text::{code_lines, CODE_LINE_CAP};

    /// Este arquivo não tem mais partes: o motor que montava a página inteira
    /// da spec saiu, e o que ficou não passa do teto de linhas de código, pela
    /// medida única do núcleo.
    #[test]
    fn the_file_does_not_go_over_the_code_line_cap() {
        let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("view").join("document").join("spec.rs");
        let source = std::fs::read_to_string(&gate).unwrap_or_default();
        assert!(code_lines(&source) <= CODE_LINE_CAP, "{}", gate.display());
    }
}
