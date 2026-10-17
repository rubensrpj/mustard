//! A declaração que a língua deixa ver fora do arquivo pelo nome escrito à
//! parte dela (`@visible.name`): `a` em `export { a }` e em
//! `module.exports = { a }`, o texto de `__all__ = ['a']`. O nome escrito ali
//! é o de uma declaração do topo do arquivo, e só ela sai marcada: a de mesmo
//! nome escrita dentro de outra declaração (o método de uma classe, a função
//! local) é outra coisa.

use std::collections::BTreeSet;
use std::ops::Range;

use tree_sitter::Node;

use super::Spans;

/// Os trechos dos nós de `decls`, cada um com o nome da declaração dele, cujo
/// nome está em `names` e que nenhum nó de outro nome contém. O de mesmo nome
/// que o contém declara o mesmo nome junto (a constante que guarda a função
/// em `const a = () => 1`), e não o tira do topo.
pub(super) fn top_declarations<'t, 'n>(decls: impl Iterator<Item = (Node<'t>, &'n str)>, names: &BTreeSet<String>) -> Spans {
    if names.is_empty() {
        return Spans::new();
    }
    let decls: Vec<(Range<usize>, &str)> = decls.map(|(node, name)| (node.byte_range(), name)).collect();
    decls
        .iter()
        .filter(|(_, name)| names.contains(*name))
        .filter(|(inner, name)| !decls.iter().any(|(outer, other)| other != name && contains(outer, inner)))
        .map(|(range, _)| (range.start, range.end))
        .collect()
}

/// `outer` contém `inner` e é maior que ele.
fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end && outer != inner
}
