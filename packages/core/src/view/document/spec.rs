//! O que a página de uma spec carrega: o estado de uma onda, o pedido
//! montado para ela, um dia da economia do rtk, e o corte da conversa grande
//! demais (`conversation`).
//!
//! O motor que montava a página inteira da spec e a do projeto a partir do
//! arquivo de eventos, item por item, saiu com o comando que só ele servia:
//! a página de uma spec e a do projeto só existem hoje como template mais
//! banco de dados.

mod conversation;

use std::collections::BTreeMap;

pub use conversation::{conversation_len, cut_oldest_conversation};

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

    /// Nenhum arquivo da página da spec passa do teto de linhas de código: a
    /// porta e cada parte da pasta dela, pela medida única do núcleo.
    #[test]
    fn no_file_of_the_spec_page_goes_over_the_code_line_cap() {
        let gate = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("view").join("document").join("spec.rs");
        assert_eq!(crate::io::fs::files_over_code_line_cap(&gate), Ok(Vec::new()));
    }
}
