//! O lugar das montagens de um arquivo visto de outro: por onde uma montagem
//! escrita noutro arquivo chega às rotas que as montagens daqui alcançam, e
//! a montagem que não leva nada a lugar nenhum.

use std::collections::HashMap;

use tree_sitter::Node;

use super::{outer_mounts, Groups, Named, Prefix};
use crate::model::Decl;

/// Um caminho inteiro de uma montagem do arquivo e os lugares em aberto da
/// montagem mais de fora dele ([`open_places`]).
pub(super) struct Chain {
    pub path: String,
    pub places: Vec<String>,
}

/// Os lugares em aberto da montagem `top`: o objeto que a recebe e a
/// declaração em que ela está escrita, quando ela nasce de um nome e nada no
/// arquivo monta nesse lugar ([`outer_mounts`]). Outro arquivo que monte um
/// deles chega às rotas que ela alcança. Nenhum para a montagem feita sobre o
/// que o arquivo mesmo faz ali (`App::new()`), que nada de fora alcança.
pub(super) fn open_places(prefixes: &[Prefix], on: &HashMap<&str, Vec<usize>>, top: usize) -> Vec<String> {
    let p = &prefixes[top];
    if !p.from_name || !outer_mounts(prefixes, on, top).is_empty() {
        return Vec::new();
    }
    let mut places: Vec<String> = [p.receiver.as_deref(), p.owner.as_deref()]
        .into_iter()
        .flatten()
        .filter(|place| !place.is_empty())
        .map(str::to_string)
        .collect();
    places.dedup();
    places
}

impl<'t> Groups<'t> {
    /// O nome escrito em `node` guarda, numa variável do arquivo, um grupo
    /// que já nasce do objeto da chamada em cujo argumento ele está
    /// (`web::scope("/api").service(v1)`, com `v1` guardado antes): o prefixo
    /// desse objeto já está no grupo, e montar o nome não leva nada a outro
    /// lugar.
    pub(super) fn held_inside(&self, node: Node<'t>, bytes: &[u8], declarations: &[Decl]) -> bool {
        matches!(
            self.named(node, bytes, declarations),
            Some(Named::Binding(binding))
                if self.outer_of(binding.value.start_byte(), bytes, declarations).is_some()
                    && self.of(binding.value, bytes, declarations, 0).is_some()
        )
    }
}

/// Um arquivo que uma montagem de outro alcança: a posição dele, se ela
/// alcança o módulo inteiro e os nomes que o alvo tem nele.
pub(super) struct Reached<'m> {
    pub file: usize,
    pub whole: bool,
    pub names: Vec<&'m str>,
}

impl Reached<'_> {
    /// `name` é um dos nomes do alvo no arquivo alcançado.
    pub(super) fn has(&self, name: &str) -> bool {
        !name.is_empty() && self.names.contains(&name)
    }
}
