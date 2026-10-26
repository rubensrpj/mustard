//! As unidades de teste do arquivo (`@test_unit`): a declaração que a língua
//! marca como teste e a chamada que um framework de teste dela roda como
//! teste. A declaração cujo nó é uma delas sai com a marca de teste do mapa
//! (`syntax.test`). Fora do arquivo de teste, as linhas dela são também trecho
//! de teste do arquivo; no arquivo de teste tudo já é teste, e a chamada que
//! ela faz segue sendo uso do que chama, porque é ela que mostra quem testa
//! cada declaração. A forma que a língua só roda no arquivo de teste
//! (`@test_unit.in_test_file`) não vale nada fora dele.

use tree_sitter::Node;

use super::Spans;

/// Onde a língua roda a unidade de teste de uma captura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Runs {
    /// Em qualquer arquivo (`@test_unit`).
    Anywhere,
    /// Só no arquivo de teste (`@test_unit.in_test_file`).
    InTestFile,
}

/// As unidades de teste de um arquivo, pelo trecho do nó de cada uma.
pub(super) struct TestUnits {
    test_file: bool,
    nodes: Spans,
}

impl TestUnits {
    /// As unidades do arquivo que é de teste pelo caminho (`test_file`) ou não.
    pub(super) fn of(test_file: bool) -> TestUnits {
        TestUnits { test_file, nodes: Spans::new() }
    }

    /// Guarda o nó de uma captura de unidade de teste que a língua roda onde
    /// `runs` diz, e responde se as linhas dele são trecho de teste do
    /// arquivo.
    pub(super) fn take(&mut self, node: Node, runs: Runs) -> bool {
        if runs == Runs::InTestFile && !self.test_file {
            return false;
        }
        self.nodes.insert((node.start_byte(), node.end_byte()));
        !self.test_file
    }

    /// O nó de uma declaração é unidade de teste.
    pub(super) fn holds(&self, node: Node) -> bool {
        self.nodes.contains(&(node.start_byte(), node.end_byte()))
    }
}
