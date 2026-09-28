//! O uso por valor: a função entregue a outra (`xs.map(dobro)`), guardada num
//! nome (`let f = dobro;`) ou ligada a um evento, sem ser chamada ali. O nome
//! escrito onde vai um valor ([`crate::model::Module::value_uses`]) liga só a
//! uma função ou a um método à vista do arquivo, e só quando o que vem escrito
//! antes dele o alcança: fora disso ele é um valor qualquer, como a variável
//! ou o campo lido, e não liga a nada.

use super::{Before, DeclId, Verdict};

/// Os tipos de declaração que um nome escrito onde vai um valor alcança: o
/// que se entrega a outra função e se chama depois.
pub(super) const KINDS: &[&str] = &["function", "method"];

/// O que o nome escrito onde vai um valor alcança, entre as declarações com
/// esse nome de [`KINDS`]:
///
/// - as dos arquivos que o caminho escrito antes dele nomeia (`named`,
///   `crate::a::triplo`), provadas;
/// - nenhuma quando o nome é de fora do projeto (`not_ours`);
/// - escrito sozinho, as que o arquivo tem à vista (`seen`), provada quando
///   é uma só;
/// - escrito depois do próprio objeto ou de um nome (`Self::metade`,
///   `calc::dobro`), as que esse qualificador estreita (`narrowed`);
/// - escrito depois de um valor (`pedido.total`), nenhuma: sem saber o tipo
///   do valor, o nome é um campo tanto quanto um método.
///
/// Nunca a família inteira da língua, como a chamada que não acha nada à
/// vista: um nome que não se chama só liga ao que o arquivo enxerga.
pub(super) fn verdict(
    named: Option<Vec<DeclId>>,
    not_ours: bool,
    before: &Before,
    seen: Vec<DeclId>,
    narrowed: impl FnOnce() -> Option<(Vec<DeclId>, bool)>,
    max_same_name: usize,
) -> Option<Verdict> {
    match (named, before) {
        (Some(files), _) => Verdict::of(files, true, max_same_name),
        (None, _) if not_ours => None,
        (None, Before::Nothing) => Verdict::of(seen, true, max_same_name),
        (None, Before::Itself | Before::Name(_)) => {
            narrowed().and_then(|(kept, provable)| Verdict::of(kept, provable, max_same_name))
        }
        (None, Before::Value) => None,
    }
}
