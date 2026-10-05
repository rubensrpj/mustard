//! O comentário escrito no fim da linha de um código
//! (`ACTIVE: 1, // ativo`) é daquela linha, e não da declaração que vem
//! embaixo: só o comentário que abre a própria linha documenta o que vem
//! depois dele.

use tree_sitter::Node;

/// O comentário `comment` começa na linha em que o nó de antes, que não é
/// comentário, termina: ele fecha a linha desse código. O comentário que
/// abre a linha, ou vem depois de outro comentário, não fecha nenhuma.
pub(super) fn closes_a_line(comment: Node) -> bool {
    comment
        .prev_sibling()
        .is_some_and(|before| !before.is_extra() && before.end_position().row == comment.start_position().row)
}
